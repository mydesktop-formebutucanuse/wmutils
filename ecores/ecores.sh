#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# wmutils: installed as /usr/local/share/wmutils/ecores/ecores.sh
# by ecores/install.sh.
#
# Keeps background work on the efficiency cores, the CPUs with less
# capacity than the largest, with two cgroups:
#   wmutils-daemons  the system's daemons: on the efficiency cores always
#   wmutils-batch    work that may take long, such as emerge's builds: on
#                    the efficiency cores while on battery, on every core
#                    while on mains power
# Run as root, except for status and idle:
#   ecores.sh start        make the cgroups and move the daemons (at boot)
#   ecores.sh power        set wmutils-batch's cores by the power supply
#                          (udev, when the charger is plugged in or out)
#   ecores.sh enter PID..  move processes into wmutils-batch
#   ecores.sh stop         put everything back and remove the cgroups
#   ecores.sh status       what is where
#   ecores.sh idle [SECS]  how deeply each core slept over SECS seconds (30)
# See ecores/README.md.
set -u

# ECORES_SYSROOT: another tree to work on than the running system's, for
# trying the script without root.
root=${ECORES_SYSROOT:-}
cg=$root/sys/fs/cgroup
cpu=$root/sys/devices/system/cpu
psy=$root/sys/class/power_supply
proc=$root/proc
daemons=wmutils-daemons
batch=wmutils-batch

die() {
	echo "$0: $*" >&2
	exit 1
}

# The efficiency cores as cpuset wants them, "0,1": nothing on a machine
# whose CPUs are all alike.
ecores() {
	for c in "$cpu"/cpu[0-9]*; do
		[ -r "$c/cpu_capacity" ] || continue
		echo "${c##*/cpu} $(cat "$c/cpu_capacity")"
	done | sort -n | awk '
		{ id[NR] = $1; cap[NR] = $2; if ($2 > max) max = $2 }
		END {
			for (i = 1; i <= NR; i++)
				if (cap[i] < max) {
					printf "%s%s", sep, id[i]
					sep = ","
				}
			print ""
		}'
}

# Is the machine running from its battery? Not if it has no mains supply
# to be unplugged from.
on_battery() {
	mains=no
	for s in "$psy"/*; do
		[ "$(cat "$s/type" 2>/dev/null)" = Mains ] || continue
		mains=yes
		[ "$(cat "$s/online" 2>/dev/null)" = 1 ] && return 1
	done
	[ "$mains" = yes ]
}

# wmutils-batch on the efficiency cores on battery; an empty cpuset.cpus is
# every core its parent has.
power() {
	[ -d "$cg/$batch" ] || return 0
	if on_battery; then
		ecores > "$cg/$batch/cpuset.cpus"
	else
		echo > "$cg/$batch/cpuset.cpus"
	fi
}

# Makes the two cgroups. Fails, with nothing made, where there are no
# efficiency cores or no cgroup v2 with cpusets.
setup() {
	e=$(ecores)
	[ -n "$e" ] || die "this machine has no efficiency cores: nothing to do"
	[ -e "$cg/cgroup.controllers" ] || die "no cgroup v2 hierarchy at $cg"
	case " $(cat "$cg/cgroup.subtree_control") " in
	*" cpuset "*) ;;
	*)
		echo +cpuset > "$cg/cgroup.subtree_control" ||
			die "the kernel has no cpuset controller"
		;;
	esac
	mkdir -p "$cg/$daemons" "$cg/$batch" || exit 1
	echo "$e" > "$cg/$daemons/cpuset.cpus" || exit 1
	power
}

# Moves every process of the root cgroup into wmutils-daemons. Those are
# the supervision tree and the daemons: a login session is in a cgroup of
# its own, where elogind puts it, and what a daemon starts later is born
# in the cgroup of the daemon. Left where they are:
# - process 1. It only waits for its children, and elogind takes the
#   cgroup of process 1 for the root under which it makes the sessions'
#   cgroups: they would be made inside wmutils-daemons.
# - kernel threads, which have no executable. Theirs is the kernel's
#   choice of CPU.
start() {
	setup
	n=0
	for pid in $(cat "$cg/cgroup.procs"); do
		[ "$pid" -eq 1 ] && continue
		readlink "$proc/$pid/exe" >/dev/null 2>&1 || continue
		# Fails for a process that has just ended.
		if echo "$pid" 2>/dev/null > "$cg/$daemons/cgroup.procs"; then
			n=$((n + 1))
		fi
	done
	echo "$n processes moved into $daemons, on CPU $(cat "$cg/$daemons/cpuset.cpus")"
}

enter() {
	[ -d "$cg/$batch" ] || setup
	for pid; do
		echo "$pid" > "$cg/$batch/cgroup.procs" || exit 1
	done
}

# Processes go back to the root cgroup, also those that entered
# wmutils-batch from a login session. A cgroup goes once it is empty: a
# process may be born in it while the others leave.
stop() {
	for g in "$daemons" "$batch"; do
		[ -d "$cg/$g" ] || continue
		try=0
		while :; do
			for pid in $(cat "$cg/$g/cgroup.procs"); do
				echo "$pid" 2>/dev/null > "$cg/cgroup.procs"
			done
			rmdir "$cg/$g" 2>/dev/null && break
			try=$((try + 1))
			if [ "$try" -ge 5 ]; then
				# Still there, but not kept to any cores.
				echo > "$cg/$g/cpuset.cpus"
				echo "$0: $cg/$g is not empty and stays, on every core" >&2
				break
			fi
			sleep 1
		done
	done
}

status() {
	e=$(ecores)
	echo "efficiency cores: ${e:-none}"
	if on_battery; then
		echo "power: battery"
	else
		echo "power: mains"
	fi
	for g in "$daemons" "$batch"; do
		if [ ! -d "$cg/$g" ]; then
			echo "$g: not there"
			continue
		fi
		echo "$g: on CPU $(cat "$cg/$g/cpuset.cpus.effective")," \
			"$(wc -l < "$cg/$g/cgroup.procs") processes"
		for pid in $(cat "$cg/$g/cgroup.procs"); do
			cat "$proc/$pid/comm" 2>/dev/null
		done | sort | uniq -c | awk '{ printf "%s%s", sep, ($1 > 1 ? $2 " x" $1 : $2); sep = ", " }
			END { if (NR) print "" }' | fold -s -w 76 | sed -e 's/ *$//' -e 's/^/  /'
	done
}

# The time since boot, then one line per idle state of every CPU: CPU,
# state, times entered, microseconds spent in it, the CPU's capacity.
idle_states() {
	echo "T $(cut -d ' ' -f 1 "$proc/uptime")"
	for c in "$cpu"/cpu[0-9]*; do
		for s in "$c"/cpuidle/state[0-9]*; do
			[ -r "$s/time" ] || continue
			echo "${c##*/cpu} ${s##*/state} $(cat "$s/usage") $(cat "$s/time")" \
				"$(cat "$c/cpu_capacity" 2>/dev/null || echo 0)"
		done
	done
}

# How often each CPU left idle and how much of the time it spent in its
# deepest idle state, over $1 seconds.
idle() {
	secs=${1:-30}
	[ "$secs" -ge 1 ] 2>/dev/null || die "idle: seconds, not \"$secs\""
	echo "Counting for $secs s; leave the machine as it is to be measured."
	{
		idle_states
		sleep "$secs"
		idle_states
	} | awk '
		# Reading the states takes time too: the seconds are those
		# between the two readings.
		$1 == "T" { secs = $2 - began; began = $2; next }
		{
			k = $1 SUBSEP $2
			if (!(k in use)) {
				use[k] = $3; time[k] = $4
				next
			}
			if (!($1 in woken)) { cpus[++n] = $1; cap[$1] = $5; deepest[$1] = -1 }
			woken[$1] += $3 - use[k]
			if ($2 > deepest[$1]) { deepest[$1] = $2; deep[$1] = $4 - time[k] }
			if ($5 > max) max = $5
		}
		# The states are not all read at the same instant.
		function pct(us) { us = us / secs / 1e4; return us > 100 ? 100 : us }
		END {
			if (!n) { print "no cpuidle states to count"; exit 1 }
			for (i = 1; i <= n; i++) {
				c = cpus[i]
				kind = cap[c] < max ? "efficiency" : (cap[c] ? "performance" : "")
				printf "cpu%-2d %-11s  left idle %6.1f times a second, %5.1f %% of the time in its deepest idle state\n",
					c, kind, woken[c] / secs, pct(deep[c])
				w[kind] += woken[c]; d[kind] += deep[c]; m[kind]++
			}
			for (kind in m)
				if (kind != "")
					printf "%s cores: left idle %.1f times a second in all, %.1f %% in the deepest state\n",
						kind, w[kind] / secs, pct(d[kind] / m[kind])
		}'
}

case ${1:-} in
start | power | stop)
	[ "$#" -eq 1 ] || die "$1 takes no arguments"
	"$1"
	;;
enter)
	shift
	[ "$#" -ge 1 ] || die "enter: which PID?"
	enter "$@"
	;;
status)
	status
	;;
idle)
	idle "${2:-}"
	;;
*)
	echo "usage: $0 start | power | enter PID... | stop | status | idle [SECONDS]" >&2
	exit 2
	;;
esac
