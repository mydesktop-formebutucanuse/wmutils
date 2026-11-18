#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# wmutils: does what the machine does every day, one thing after another,
# and says for each whether it worked and what SELinux refused it (or, in
# permissive mode, would have refused).
#
#   sudo sh selinux/trial.sh [-e | -p] [-t SECONDS] [-o DIR] [TEST...]
#   sh selinux/trial.sh -l        the tests
#
# Without -e or -p SELinux stays as it is. With -e the script switches to
# enforcing for the tests and back when it ends, however it ends: on a
# signal, when its terminal goes, and after SECONDS (900) from a process
# of its own if the script itself is stuck. With -p it switches to
# permissive for the tests, to see all that enforcing would refuse, and
# back when it ends (not after SECONDS: permissive is the way back).
#
# The report and the kernel's lines go to DIR (./selinux-trial-DATE).
# See selinux/README.md.
set -u

here=$(cd "$(dirname "$0")" && pwd)
all='clients power udev services wifi bluetooth sound vt semodule emerge suspend'
# Only when named: it writes the kernel image the machine boots from.
optional='kernel'
esp=/boot/efi
services='chronyd bluetoothd metalog rclone getty-tty2 NetworkManager'
enforce=/sys/fs/selinux/enforce

die() {
	echo "$0: $*" >&2
	exit 1
}

describe() {
	cat <<'EOF'
clients    the user asks elogind, NetworkManager, bluetoothd and chronyd something
power      udev runs ecores.sh for a change of the power supply
udev       udev's rules again for the backlight, the LEDs, input and sound
services   s6 restarts chronyd, bluetoothd, metalog, rclone, a getty, NetworkManager
wifi       Wi-Fi off and on, and the connection back
bluetooth  Bluetooth off and on (or on and off)
sound      the user plays half a second of a quiet tone
vt         to another console and back, which takes X's devices away and returns them
semodule   the policy built and loaded again
emerge     a small package built and merged, in each of Portage's build directories
suspend    to sleep through elogind; a key wakes the machine
manual     (with -e, at a terminal) sudo and a login on another console, by hand
kernel     (only when named) emerge --config sys-kernel/asahi-kernel: the
           initramfs, the UKI, BOOTAA64.EFI and m1n1's boot.bin on the ESP
EOF
}

# --- the kernel's log -------------------------------------------------

# A line of ours in the kernel's log: what divides it into tests.
mark() {
	echo "wmutils-trial $id: $*" >/dev/kmsg
}

# Waits until the kernel's log as we copy it has the line.
seen() {
	n=0
	while ! grep -aqF "wmutils-trial $id: $*" "$out/kmsg" 2>/dev/null; do
		n=$((n + 1))
		[ "$n" -le 30 ] || return 1
		sleep 0.1
	done
}

# The denials since the start, each with the test it fell into.
denials() {
	awk -v id="wmutils-trial $id: " '
		# A record is "priority,number,time,flags;text", and what
		# follows it indented is not ours. The same record twice,
		# where the copy had to start again, counts once.
		/^ / { next }
		{
			split($0, head, /[,;]/)
			if (head[2] in had) next
			had[head[2]] = 1
			msg = $0
			sub(/^[^;]*;/, "", msg)
		}
		index(msg, id) == 1 {
			what = substr(msg, length(id) + 1)
			if (what == "start") on = 1
			else if (what ~ /^begin /) cur = substr(what, 7)
			else if (what ~ /^end /) cur = ""
			next
		}
		on && /avc: / { print "[" (cur == "" ? "-" : cur) "] " msg }
	' "$out/kmsg"
}

# --- helpers ----------------------------------------------------------

as_user() {
	runuser -u "$user" -- env XDG_RUNTIME_DIR="/run/user/$uid" \
		DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"
}

label() {
	tr -d '\0\n' <"/proc/$1/attr/current" 2>/dev/null
}

# The type of a label.
type_of() {
	echo "$1" | cut -d: -f3
}

# The network back, if there was one.
net_back() {
	[ "$net" = connected ] || return 0
	n=0
	while [ "$(nmcli -t -f STATE general 2>/dev/null)" != connected ]; do
		n=$((n + 1))
		if [ "$n" -gt 60 ]; then
			echo "NetworkManager is not connected again after 60 seconds"
			return 1
		fi
		sleep 1
	done
	echo "NetworkManager is connected again"
}

# PORTAGE_TMPDIR where /etc/portage sets another than /var/tmp.
tmpdirs() {
	grep -rhs '^[[:space:]]*PORTAGE_TMPDIR=' /etc/portage/make.conf /etc/portage/env |
		sed 's/^[^=]*=//; s/[[:space:]]*#.*$//; s/^["'\'']//; s/["'\'']$//; s:/*$::' |
		sort -u | grep '^/' | grep -vx -e /var/tmp -e /tmp
}

# --- the tests: 0 it worked, 1 it did not, 77 not tried ----------------

t_clients() {
	rc=0
	echo "the user is $(as_user id -Z)"
	as_user timeout 15 loginctl list-sessions || rc=1
	as_user timeout 15 nmcli -t general status || rc=1
	as_user timeout 15 bluetoothctl show >/dev/null || rc=1
	timeout 15 chronyc -n tracking >/dev/null || rc=1
	return $rc
}

t_power() {
	f=/sys/fs/cgroup/wmutils-batch/cpuset.cpus
	[ -e "$f" ] || { echo "no $f: ecores is not installed"; return 77; }
	was=$(cat "$f")
	# The other of the two values ecores.sh sets, for udev to put right.
	if [ -n "$was" ]; then echo >"$f"; else echo 0 >"$f"; fi
	echo "cpuset.cpus was '$was', set to '$(cat "$f")'"
	udevadm trigger --action=change --subsystem-match=power_supply
	udevadm settle --timeout=10
	sleep 1
	now=$(cat "$f")
	echo "after the event: '$now'"
	[ "$now" = "$was" ] && return 0
	echo "$was" >"$f"
	echo "udev did not run ecores.sh, or it could not write"
	return 1
}

t_udev() {
	was=$(stat -c '%a %U %G %n' /sys/class/backlight/*/brightness 2>/dev/null)
	for s in backlight leds input sound; do
		udevadm trigger --action=change --subsystem-match="$s" || return 1
	done
	udevadm settle --timeout=30 || { echo "udev did not settle"; return 1; }
	now=$(stat -c '%a %U %G %n' /sys/class/backlight/*/brightness 2>/dev/null)
	echo "$now"
	[ "$now" = "$was" ] || { echo "the brightness file was: $was"; return 1; }
}

t_services() {
	rc=0
	for s in $services; do
		d=/run/service/$s
		[ -d "$d" ] || continue
		pid0=$(s6-svstat -o pid "$d" 2>/dev/null) || continue
		[ "$pid0" -gt 0 ] 2>/dev/null || { echo "$s: not up, left alone"; continue; }
		lab0=$(label "$pid0")
		# A getty that somebody has logged in on is that login.
		case $s in
		getty-*)
			if [ "$(type_of "$lab0")" != getty_t ]; then
				echo "$s: in use ($(type_of "$lab0")), left alone"
				continue
			fi
			;;
		esac
		fuse0=$(findmnt -n -t fuse.rclone 2>/dev/null | wc -l)
		s6-svc -r "$d"
		n=0 pid=-1
		while :; do
			sleep 1
			pid=$(s6-svstat -o pid "$d" 2>/dev/null) || pid=-1
			[ "$pid" -gt 0 ] 2>/dev/null && [ "$pid" != "$pid0" ] && break
			n=$((n + 1))
			[ "$n" -lt 30 ] || break
		done
		# Still the same process a little later: it did not start and die.
		sleep 3
		if [ "$pid" = "$pid0" ] || [ "$(s6-svstat -o pid "$d" 2>/dev/null)" != "$pid" ]; then
			echo "$s: did not come up again (was $(type_of "$lab0"))"
			rc=1
			continue
		fi
		lab=$(label "$pid")
		if [ "$(type_of "$lab")" != "$(type_of "$lab0")" ]; then
			echo "$s: up again as $(type_of "$lab"), was $(type_of "$lab0")"
			rc=1
			continue
		fi
		echo "$s: up again as $(type_of "$lab")"
		case $s in
		rclone)
			n=0
			while [ "$(findmnt -n -t fuse.rclone 2>/dev/null | wc -l)" -lt "$fuse0" ]; do
				n=$((n + 1))
				if [ "$n" -gt 30 ]; then
					echo "rclone: its mount is not back"
					rc=1
					break
				fi
				sleep 1
			done
			;;
		NetworkManager) net_back || rc=1 ;;
		esac
	done
	return $rc
}

t_wifi() {
	[ "$(nmcli radio wifi 2>/dev/null)" = enabled ] || { echo "Wi-Fi is off"; return 77; }
	nmcli radio wifi off || return 1
	sleep 3
	nmcli radio wifi on || return 1
	net_back
}

t_bluetooth() {
	was=$(timeout 10 bluetoothctl show 2>/dev/null | awk '$1 == "Powered:" { print $2 }')
	[ -n "$was" ] || { echo "no Bluetooth controller"; return 77; }
	if [ "$was" = yes ]; then first=off second=on; else first=on second=off; fi
	timeout 20 bluetoothctl power "$first" || return 1
	sleep 2
	timeout 20 bluetoothctl power "$second" || return 1
	sleep 1
	now=$(timeout 10 bluetoothctl show 2>/dev/null | awk '$1 == "Powered:" { print $2 }')
	echo "Powered: $now"
	[ "$now" = "$was" ]
}

t_sound() {
	python3 - "$out/tone.wav" <<'EOF' || return 77
import math, struct, sys, wave
w = wave.open(sys.argv[1], 'wb')
w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
n = 24000
for i in range(n):
    fade = min(1.0, i / 2400, (n - i) / 2400)
    v = int(1200 * fade * math.sin(2 * math.pi * 440 * i / 48000))
    w.writeframes(struct.pack('<hh', v, v))
w.close()
EOF
	chmod 644 "$out/tone.wav"
	as_user timeout 20 pw-play "$out/tone.wav"
}

t_vt() {
	cur=$(fgconsole 2>/dev/null) || { echo "no console"; return 77; }
	if [ "$cur" = 2 ]; then other=3; else other=2; fi
	x=$(pgrep -x -o Xorg || pgrep -x -o X) || x=
	sid=$(loginctl list-sessions --no-legend 2>/dev/null | awk -v u="$user" '$3 == u { print $1; exit }')
	chvt "$other" || return 1
	sleep 2
	chvt "$cur" || return 1
	sleep 3
	if [ -n "$x" ] && [ ! -d "/proc/$x" ]; then
		echo "the X server is gone"
		return 1
	fi
	if [ -n "$sid" ]; then
		act=$(loginctl show-session "$sid" -p Active --value 2>/dev/null)
		echo "session $sid active: $act"
		[ "$act" = yes ] || return 1
	fi
}

t_semodule() {
	semodule -B
}

t_emerge() {
	if pgrep -f 'python-exec/[^ ]*/emerge ' >/dev/null; then
		echo "another emerge is running"
		return 77
	fi
	atom=
	for p in sys-apps/which app-arch/cpio sys-apps/less; do
		for v in /var/db/pkg/"$p"-[0-9]*; do
			[ -d "$v" ] && atom="=${v#/var/db/pkg/}" && break 2
		done
	done
	[ -n "$atom" ] || { echo "none of the small packages is installed"; return 77; }
	rc=0
	for d in "" $(tmpdirs); do
		echo "--- emerge $atom${d:+ with PORTAGE_TMPDIR=$d}"
		if [ -n "$d" ]; then
			PORTAGE_TMPDIR=$d timeout 900 emerge --oneshot --nodeps --getbinpkg=n --usepkg=n --quiet-build=y "$atom" </dev/null || rc=1
		else
			timeout 900 emerge --oneshot --nodeps --getbinpkg=n --usepkg=n --quiet-build=y "$atom" </dev/null || rc=1
		fi
	done
	ls -Z /etc/ld.so.cache
	return $rc
}

t_suspend() {
	echo "The machine goes to sleep in 5 seconds. Wake it with a key." >/dev/tty 2>/dev/null
	sleep 5
	loginctl suspend || return 1
	n=0
	until grep -a -A100000 -F "wmutils-trial $id: begin suspend" "$out/kmsg" | grep -aq 'PM: suspend exit'; do
		n=$((n + 1))
		if [ "$n" -gt 600 ]; then
			echo "no sleep and wake in the kernel's log after 10 minutes"
			return 1
		fi
		sleep 1
	done
	echo "slept and woke"
	sleep 3
	net_back
}

t_manual() {
	[ -r /dev/tty ] || return 77
	cat >/dev/tty <<'EOF'

SELinux is enforcing now. By hand, while this waits:
  1. in another terminal:  sudo -k; sudo true
  2. Ctrl+Alt+F2, log in, log out, and back with Ctrl+Alt+F1 (or where X is)
Did both work? [y/n]
EOF
	read -r a </dev/tty || return 77
	case $a in
	y* | Y*) return 0 ;;
	*)
		echo "answered: $a"
		return 1
		;;
	esac
}

# What the kernel's install writes on the ESP: the UKIs, the copy U-Boot
# boots, m1n1's stage 2.
esp_files() {
	ls -l --time-style=+%F_%T "$esp"/EFI/Linux/*.efi "$esp/EFI/BOOT/BOOTAA64.EFI" \
		"$esp/m1n1/boot.bin" 2>&1
	sha256sum "$esp/EFI/BOOT/BOOTAA64.EFI" "$esp/m1n1/boot.bin" 2>&1
}

# Every file the install writes on the ESP is written beside the old one
# and renamed over it (installkernel keeps the old UKI as *-old.efi, the
# hooks write *.new and mv), so a step SELinux refuses leaves the old
# image: enforcing, the test may run.
t_kernel() {
	if pgrep -f 'python-exec/[^ ]*/emerge ' >/dev/null; then
		echo "another emerge is running"
		return 77
	fi
	ver=$(uname -r)
	echo "--- modules in /lib/modules/$ver/updates (the initramfs takes those it needs):"
	ls -l "/lib/modules/$ver/updates" 2>&1
	echo "--- the ESP before"
	esp_files
	echo "--- emerge --config sys-kernel/asahi-kernel"
	timeout 1800 emerge --config sys-kernel/asahi-kernel </dev/null >"$out/.kernel" 2>&1
	st=$?
	cat "$out/.kernel"
	echo "--- the ESP after"
	esp_files
	# The last reason is the one that matters most.
	why=
	grep -q 'm1n1 updated at ' "$out/.kernel" || why="update-m1n1 did not finish"
	grep -q '95-uki-to-bootaa64: .* <- ' "$out/.kernel" || why="95-uki-to-bootaa64 did not copy the UKI"
	[ "$st" = 0 ] || why="emerge --config ended with $st"
	if [ "$e" = yes ] && [ "$(cat "$enforce")" != 1 ]; then
		why="SELinux was permissive by the end (the -t limit?): not a trial under enforcing"
	fi
	rm -f "$out/.kernel"
	[ -n "$why" ] || return 0
	echo "$why"
	return 1
}

# --- running them -----------------------------------------------------

# Puts everything back. Run at the end, whatever the end is.
cleanup() {
	trap - EXIT INT TERM HUP
	if [ -n "${mode0:-}" ] && [ "$(cat "$enforce" 2>/dev/null)" != "$mode0" ]; then
		echo "$mode0" >"$enforce"
		echo "SELinux is $(mode_name "$(cat "$enforce")") again."
	fi
	# It ignores the signals a terminal sends; its sleep ends by itself.
	if [ -n "${watchdog:-}" ]; then
		{
			kill -KILL "$watchdog"
			wait "$watchdog"
		} 2>/dev/null
	fi
	[ -z "${rate0:-}" ] || echo "$rate0" 2>/dev/null >/proc/sys/kernel/printk_ratelimit
	[ -z "${devkmsg0:-}" ] || echo "$devkmsg0" 2>/dev/null >/proc/sys/kernel/printk_devkmsg
	if [ -n "${copier:-}" ]; then
		sleep 1
		kill "$copier" 2>/dev/null
	fi
	[ ! -e "${out:-}/kmsg" ] || report
}

report() {
	denials >"$out/denials.txt"
	{
		echo "SELinux trial, $(date '+%Y-%m-%d %H:%M:%S'), $(if [ "$e" = yes ]; then echo enforcing; elif [ "$p" = yes ]; then echo permissive; else echo "as it was ($(mode_name "$mode0"))"; fi)"
		echo
		printf '%-10s %-6s %7s %8s\n' test result seconds denials
		while read -r name res secs; do
			printf '%-10s %-6s %7s %8s\n' "$name" "$res" "$secs" \
				"$(grep -c "^\[$name\] " "$out/denials.txt")"
		done <"$out/results"
		printf '%-10s %-6s %7s %8s\n' "(between)" "" "" "$(grep -c '^\[-\] ' "$out/denials.txt")"
		echo
		if [ -s "$out/denials.txt" ]; then
			sh "$here/install.sh" denials "$out/denials.txt" | sed '$d'
		else
			echo "No denials."
		fi
		echo
		echo "Each test's own output: $out/<test>.log; the kernel's lines: $out/denials.txt"
	} >"$out/report.txt" 2>&1
	rm -f "$out/tone.wav"
	chown -R "$uid" "$out" 2>/dev/null
	cat "$out/report.txt"
}

mode_name() {
	if [ "$1" = 1 ]; then echo enforcing; else echo permissive; fi
}

run() {
	name=$1
	log=$out/$name.log
	# Permissive, the kernel logs a thing once and remembers: loading the
	# policy again has it forget, so each test has its own denials.
	[ "$(cat "$enforce")" = 1 ] || load_policy 2>/dev/null
	mark "begin $name"
	printf '%-10s ' "$name"
	t0=$(date +%s)
	# Through a pipe: a confined program that a test runs (semodule,
	# setfiles) may write to a pipe of ours, not to a file in a home.
	{
		"t_$name"
		echo $? >"$out/.rc"
	} 2>&1 | cat >"$log"
	rc=$(cat "$out/.rc" 2>/dev/null) || rc=1
	rm -f "$out/.rc"
	secs=$(($(date +%s) - t0))
	mark "end $name"
	seen "end $name"
	case $rc in
	0) res=ok ;;
	77) res=skip ;;
	*) res=FAILED ;;
	esac
	echo "$name $res $secs" >>"$out/results"
	echo "$res${secs:+ (${secs}s)}$(if [ "$rc" != 0 ]; then echo ": $(tail -n 1 "$log")"; fi)"
}

main() {
	e=no p=no limit=900 out=
	while getopts eplt:o: o; do
		case $o in
		e) e=yes ;;
		p) p=yes ;;
		l)
			describe
			exit 0
			;;
		t) limit=$OPTARG ;;
		o) out=$OPTARG ;;
		*) die "usage: $0 [-e | -p] [-t SECONDS] [-o DIR] [TEST...]" ;;
		esac
	done
	shift $((OPTIND - 1))
	[ "$e" = no ] || [ "$p" = no ] || die "-e or -p, not both"
	[ "$(id -u)" -eq 0 ] || die "Run as root: sudo sh $0"
	[ -e "$enforce" ] || die "SELinux is not on (no $enforce)."
	[ "$limit" -gt 0 ] 2>/dev/null || die "-t wants seconds"
	tests=${*:-$all}
	for t in $tests; do
		case " $all $optional manual " in
		*" $t "*) ;;
		*) die "no test '$t': sh $0 -l" ;;
		esac
	done
	if [ "$e" = yes ] && [ $# -eq 0 ]; then
		tests="$tests manual"
	fi

	user=${SUDO_USER:-}
	[ -n "$user" ] && [ "$user" != root ] ||
		user=$(loginctl list-sessions --no-legend 2>/dev/null | awk '$2 >= 1000 { print $3; exit }')
	[ -n "$user" ] || die "Whose session? Run it with sudo from the user's terminal."
	uid=$(id -u "$user") || exit 1
	net=$(nmcli -t -f STATE general 2>/dev/null)

	: "${out:=$PWD/selinux-trial-$(date +%Y%m%d-%H%M%S)}"
	mkdir -p "$out" || exit 1
	chmod 755 "$out"
	: >"$out/results"
	id=$$-$(date +%s)
	mode0=$(cat "$enforce")

	trap cleanup EXIT
	trap 'echo; echo "interrupted"; exit 130' INT TERM HUP

	# Every line, also when there are many: the kernel's limits off.
	rate0=$(cat /proc/sys/kernel/printk_ratelimit 2>/dev/null)
	echo 0 2>/dev/null >/proc/sys/kernel/printk_ratelimit
	devkmsg0=$(cat /proc/sys/kernel/printk_devkmsg 2>/dev/null)
	echo on 2>/dev/null >/proc/sys/kernel/printk_devkmsg
	# cat ends when the kernel drops a record it had not read yet: again
	# then, from the oldest the kernel still has.
	(
		trap 'kill "$c" 2>/dev/null; exit 0' TERM
		while :; do
			cat /dev/kmsg 2>/dev/null &
			c=$!
			wait "$c"
			sleep 0.2
		done
	) >"$out/kmsg" &
	copier=$!
	mark start
	seen start || die "Cannot follow the kernel's log (/dev/kmsg)."

	if [ "$e" = yes ] && [ "$mode0" != 1 ]; then
		# The way back that does not need this script to be alive.
		(
			trap '' HUP INT TERM
			sleep "$limit"
			echo 0 >"$enforce"
		) </dev/null >/dev/null 2>&1 &
		watchdog=$!
		echo 1 >"$enforce" || die "Could not switch to enforcing."
		echo "SELinux is enforcing for the tests, and for $limit seconds at most."
	elif [ "$p" = yes ] && [ "$mode0" != 0 ]; then
		echo 0 >"$enforce" || die "Could not switch to permissive."
		echo "SELinux is permissive for the tests, and $(mode_name "$mode0") again when they end."
	else
		echo "SELinux is $(mode_name "$mode0") and stays so."
	fi

	for t in $tests; do
		run "$t"
	done
	echo
}

[ -n "${TRIAL_SOURCED:-}" ] || main "$@"
