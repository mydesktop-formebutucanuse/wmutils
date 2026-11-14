#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Measure what polling the keyboard and trackpad's SPI bus changes
# (patch 0012): read them by interrupt and by polling in turns, and
# compare the interrupts, the wakeups and the machine's power draw.
# Run as root, on battery, and keep one finger circling on the trackpad
# from the first line to the last, the same way throughout:
#   sudo sh kernel/trackpad-ab.sh [pairs [seconds]]
# pairs    how many interrupt/polling pairs to measure (4)
# seconds  the length of each half of a pair (20); 2 s more go before
#          each, uncounted
# The halves go irq-poll, poll-irq, irq-poll, ... so that a slow drift
# cancels out. At the end, and when interrupted, poll_us is back where it
# was.
set -eu

pairs=${1:-4}
secs=${2:-20}
settle=2
# Half a FIFO of 8-bit words at 8 MHz takes 8 us: above the first, at the
# second.
irq_us=5
poll_us=8

param=/sys/module/spi_apple/parameters/poll_us
battery=/sys/class/power_supply/macsmc-battery

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 $*" >&2
	exit 1
fi
if ! [ "$pairs" -ge 1 ] 2>/dev/null || ! [ "$secs" -ge 5 ] 2>/dev/null; then
	echo "usage: sudo sh $0 [pairs [seconds]]   (seconds 5 or more)" >&2
	exit 2
fi
if [ ! -e "$param" ]; then
	echo "spi_apple has no poll_us: this kernel is without 0012." >&2
	exit 1
fi

sensor=
for l in /sys/class/hwmon/hwmon*/power*_label; do
	[ -e "$l" ] || continue
	if [ "$(cat "$l")" = "Total System Power" ]; then
		sensor=${l%_label}_input
	fi
done
if [ -z "$sensor" ]; then
	echo "No \"Total System Power\" sensor (macsmc_hwmon)." >&2
	exit 1
fi

# The thread that reads the packets.
tid=
for d in /proc/[0-9]*; do
	c=$(cat "$d/comm" 2>/dev/null) || continue
	case "$c" in
	irq/*-spi-hid*) tid=${d#/proc/} ;;
	esac
done
if [ -z "$tid" ]; then
	echo "No irq/N-spi-hid-apple-irq thread: no keyboard and trackpad on SPI." >&2
	exit 1
fi

# The performance cores are the CPUs of the largest capacity.
max=0
for c in /sys/devices/system/cpu/cpu[0-9]*/cpu_capacity; do
	v=$(cat "$c")
	[ "$v" -gt "$max" ] && max=$v
done
pcpus=
for c in /sys/devices/system/cpu/cpu[0-9]*/cpu_capacity; do
	n=${c%/cpu_capacity}
	[ "$(cat "$c")" -eq "$max" ] && pcpus="$pcpus ${n##*/cpu}"
done

if [ "$(cat "$battery/status" 2>/dev/null)" != Discharging ]; then
	echo "Not on battery: the reading then has the charger's doings in it." >&2
fi

tmp=$(mktemp -d)
saved=$(cat "$param")
restore() {
	echo "$saved" > "$param"
	rm -rf "$tmp"
}
trap restore EXIT
trap 'exit 130' INT TERM

# The interrupt counts, the thread's sleeps and every CPU's idle entries.
snap() {
	cat /proc/interrupts > "$1.irq"
	awk '/^voluntary_ctxt_switches/ { print $2 }' "/proc/$tid/status" > "$1.ctx"
	for c in /sys/devices/system/cpu/cpu[0-9]*; do
		n=0
		for u in "$c"/cpuidle/state*/usage; do
			n=$((n + $(cat "$u")))
		done
		echo "${c##*/cpu} $n"
	done > "$1.idle"
}

# One half: set poll_us, wait, count for $secs seconds with a power
# reading a second. Prints the half's line and adds it to $tmp/rows.
half() {
	echo "$2" > "$param"
	sleep "$settle"
	snap "$tmp/a"
	power=0
	i=0
	while [ "$i" -lt "$secs" ]; do
		sleep 1
		power=$((power + $(cat "$sensor")))
		i=$((i + 1))
	done
	snap "$tmp/b"
	awk -v pcpus="$pcpus" -v secs="$secs" -v power="$power" -v mode="$1" \
	    -v ctx="$(($(cat "$tmp/b.ctx") - $(cat "$tmp/a.ctx")))" \
	    -v count="$count" -v halves="$((pairs * 2))" -v rows="$tmp/rows" '
		BEGIN {
			n = split(pcpus, p, " ")
			for (i = 1; i <= n; i++)
				isp[p[i]] = 1
		}
		FILENAME ~ /\.irq$/ {
			sign = FILENAME == ARGV[1] ? -1 : 1
			if (FNR == 1) {
				ncpu = NF
				next
			}
			if ($NF ~ /\.spi$/) {
				for (i = 1; i <= ncpu; i++) {
					spi += sign * $(i + 1)
					if ((i - 1) in isp)
						spip += sign * $(i + 1)
				}
			} else if ($NF == "spi-hid-apple-irq") {
				for (i = 1; i <= ncpu; i++)
					pkt += sign * $(i + 1)
			}
			next
		}
		{
			sign = FILENAME == ARGV[3] ? -1 : 1
			if ($1 in isp)
				pidle += sign * $2
			else
				eidle += sign * $2
		}
		END {
			printf "%3d/%d  %-4s %6.0f %7.0f %5.0f %% %8.0f %8.0f %8.0f %7.3f W%s\n",
				count, halves, mode, pkt / secs, spi / secs,
				spi ? 100 * spip / spi : 0, ctx / secs,
				pidle / secs, eidle / secs, power / secs / 1e6,
				pkt / secs < 20 ? "  (hardly a finger on it)" : ""
			print mode, pkt / secs, spi / secs, ctx / secs, pidle / secs, \
				eidle / secs, power / secs / 1e6 >> rows
		}' "$tmp/a.irq" "$tmp/b.irq" "$tmp/a.idle" "$tmp/b.idle"
}

echo "$pairs pairs, $secs s in each half: about $((pairs * 2 * (secs + settle) / 60 + 1)) min." \
	"Keep one finger circling on the trackpad."
echo "           packets SPI irq    on P   thread  P cores  E cores"
echo "                /s      /s   cores sleeps/s  idles/s  idles/s     power"

count=0
pair=0
while [ "$pair" -lt "$pairs" ]; do
	if [ $((pair % 2)) -eq 0 ]; then
		order="irq poll"
	else
		order="poll irq"
	fi
	for mode in $order; do
		count=$((count + 1))
		if [ "$mode" = irq ]; then
			half irq "$irq_us"
		else
			half poll "$poll_us"
		fi
	done
	pair=$((pair + 1))
done

awk '
	{
		n[$1]++
		for (i = 2; i <= 7; i++)
			s[$1, i] += $i
		if ($1 == "irq")
			irq[n[$1]] = $7
		else
			poll[n[$1]] = $7
	}
	END {
		print ""
		for (m = 1; m <= 2; m++) {
			k = m == 1 ? "irq" : "poll"
			printf "%-4s  %.0f packets, %.0f SPI interrupts, %.0f thread sleeps,", \
				k, s[k, 2] / n[k], s[k, 3] / n[k], s[k, 4] / n[k]
			printf " %.0f P-core and %.0f E-core idles a second, %.3f W\n", \
				s[k, 5] / n[k], s[k, 6] / n[k], s[k, 7] / n[k]
		}
		np = n["irq"]
		for (i = 1; i <= np; i++) {
			d = irq[i] - poll[i]
			sum += d
			sq += d * d
		}
		mean = sum / np
		se = np > 1 ? sqrt((sq - np * mean * mean) / (np - 1) / np) : 0
		printf "Polling saves %.3f W", mean
		if (np > 1)
			printf ", give or take %.3f (standard error)", se
		print "."
		if (np > 1 && (mean < 0 ? -mean : mean) < 2 * se)
			print "That is not told apart from nothing; the finger is not that even."
	}' "$tmp/rows"
