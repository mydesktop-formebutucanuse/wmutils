#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Measure what the "sleep while nothing is plugged in" switches (patches
# 0007 to 0011) save: turn them off and on in turns and compare the
# machine's power draw, read from the SMC once a second.
# Run as root, on battery, with nothing plugged in, and leave the machine
# alone until it is done:
#   sudo sh kernel/unplugged-ab.sh [pairs [seconds [parts]]]
# pairs    how many off/on pairs to measure (6)
# seconds  the length of each half of a pair; its first 10 s, in which
#          the parts wake up or go to sleep, are not counted (60)
# parts    the switches to flip, any of: sd hdmi hdmiphy usb i2c
#          ("sd hdmi usb"). hdmi flips the converter, and the port's PHY
#          and crossbar with it if hdmi_idle_phy_off is on; hdmiphy flips
#          those two alone, with the converter left as it is; i2c flips
#          the four I2C buses, which sleep between transfers
# The halves go off-on, on-off, off-on, ... so that a slow drift (the
# battery's voltage, the room) cancels out. At the end, and when
# interrupted, every switch is back where it was.
set -eu

pairs=${1:-6}
secs=${2:-60}
parts=${3:-sd hdmi usb}
settle=10

plat=/sys/bus/platform/devices
usb="702280000.usb 703000000.phy b02280000.usb b03000000.phy f02280000.usb f03000000.phy"
sd=/sys/bus/pci/devices/0000:02:00.0
sdparam=/sys/module/sdhci_pci/parameters/gl9755_runtime_pm
hdmi=/sys/module/appledrm/parameters/hdmi_idle_off
hdmiphy=/sys/module/appledrm/parameters/hdmi_idle_phy_off
hdmipath="1303000000.phy 130304c000.mux"
i2c="39b040000.i2c 39b044000.i2c 39b048000.i2c 39b04c000.i2c"
battery=/sys/class/power_supply/macsmc-battery

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 $*" >&2
	exit 1
fi
if ! [ "$pairs" -ge 1 ] 2>/dev/null || ! [ "$secs" -gt "$settle" ] 2>/dev/null; then
	echo "usage: sudo sh $0 [pairs [seconds [parts]]]   (seconds above $settle)" >&2
	exit 2
fi

has() {
	case " $parts " in
	*" $1 "*) return 0 ;;
	esac
	return 1
}

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

# A switch that is not there would be measured as saving nothing.
for p in $parts; do
	case "$p" in
	sd)
		if [ "$(cat "$sdparam" 2>/dev/null)" != Y ]; then
			echo "sd: sdhci_pci.gl9755_runtime_pm is not on (0007, unplugged.conf)." >&2
			exit 1
		fi
		;;
	hdmi)
		if [ ! -e "$hdmi" ]; then
			echo "hdmi: no appledrm.hdmi_idle_off (0008)." >&2
			exit 1
		fi
		;;
	hdmiphy)
		if [ ! -e "$hdmiphy" ]; then
			echo "hdmiphy: no appledrm.hdmi_idle_phy_off (0010)." >&2
			exit 1
		fi
		if ! has hdmi && [ "$(cat "$hdmi" 2>/dev/null)" != Y ]; then
			echo "hdmiphy: does nothing without appledrm.hdmi_idle_off (0008, unplugged.conf)." >&2
			exit 1
		fi
		for d in $hdmipath; do
			if [ "$(cat "$plat/$d/power/control" 2>/dev/null)" != auto ]; then
				echo "hdmiphy: $d may not sleep, its power/control is not auto (unplugged.rules)." >&2
				exit 1
			fi
		done
		;;
	usb)
		for d in $usb; do
			if [ "$(cat "$plat/$d/power/runtime_status" 2>/dev/null)" = unsupported ] ||
			   [ ! -e "$plat/$d/power/control" ]; then
				echo "usb: $d has no runtime PM (0009)." >&2
				exit 1
			fi
		done
		;;
	i2c)
		for d in $i2c; do
			if [ "$(cat "$plat/$d/power/runtime_status" 2>/dev/null)" = unsupported ] ||
			   [ ! -e "$plat/$d/power/control" ]; then
				echo "i2c: $d has no runtime PM (0011)." >&2
				exit 1
			fi
		done
		;;
	*)
		echo "parts are sd, hdmi, hdmiphy, usb and i2c, not \"$p\"." >&2
		exit 2
		;;
	esac
done

if [ "$(cat "$battery/status" 2>/dev/null)" != Discharging ]; then
	echo "Not on battery: the reading then has the charger's doings in it." >&2
fi

results=$(mktemp)
saved_hdmi=$(cat "$hdmi" 2>/dev/null || true)
saved_hdmiphy=$(cat "$hdmiphy" 2>/dev/null || true)
saved_sd=$(cat "$sd/power/control")
saved_usb=
for d in $usb; do
	saved_usb="$saved_usb $(cat "$plat/$d/power/control" 2>/dev/null || echo on)"
done
saved_i2c=
for d in $i2c; do
	saved_i2c="$saved_i2c $(cat "$plat/$d/power/control" 2>/dev/null || echo on)"
done

restore() {
	trap - EXIT INT TERM
	rm -f "$results"
	if has hdmi; then
		echo "$saved_hdmi" > "$hdmi"
	fi
	if has hdmiphy; then
		echo "$saved_hdmiphy" > "$hdmiphy"
	fi
	if has sd; then
		echo "$saved_sd" > "$sd/power/control"
	fi
	if has usb; then
		# shellcheck disable=SC2086
		set -- $saved_usb
		for d in $usb; do
			echo "$1" > "$plat/$d/power/control"
			shift
		done
	fi
	if has i2c; then
		# shellcheck disable=SC2086
		set -- $saved_i2c
		for d in $i2c; do
			echo "$1" > "$plat/$d/power/control"
			shift
		done
	fi
	echo "The switches are back where they were."
}
trap restore EXIT
trap 'exit 130' INT TERM

# on: the parts may sleep. off: they stay powered, as without the patches.
flip() {
	if [ "$1" = on ]; then
		control=auto
		idle_off=1
	else
		control=on
		idle_off=0
	fi
	# A fault in a part that is asleep stops the machine: disks first.
	sync
	if has hdmi; then
		echo "$idle_off" > "$hdmi"
	fi
	if has hdmiphy; then
		echo "$idle_off" > "$hdmiphy"
	fi
	if has sd; then
		echo "$control" > "$sd/power/control"
	fi
	if has usb; then
		for d in $usb; do
			echo "$control" > "$plat/$d/power/control"
		done
	fi
	if has i2c; then
		for d in $i2c; do
			echo "$control" > "$plat/$d/power/control"
		done
	fi
}

# How many of the flipped devices are asleep, as "3/7". The HDMI converter
# has no status to read and is not counted.
asleep() {
	n=0
	all=0
	if has hdmiphy; then
		for d in $hdmipath; do
			all=$((all + 1))
			[ "$(cat "$plat/$d/power/runtime_status")" = suspended ] && n=$((n + 1))
		done
	fi
	if has sd; then
		all=$((all + 1))
		[ "$(cat "$sd/power/runtime_status")" = suspended ] && n=$((n + 1))
	fi
	if has usb; then
		for d in $usb; do
			all=$((all + 1))
			[ "$(cat "$plat/$d/power/runtime_status")" = suspended ] && n=$((n + 1))
		done
	fi
	if has i2c; then
		for d in $i2c; do
			all=$((all + 1))
			[ "$(cat "$plat/$d/power/runtime_status")" = suspended ] && n=$((n + 1))
		done
	fi
	echo "$n/$all"
}

cpu() {
	awk '/^cpu /{print $2+$3+$4+$5+$6+$7+$8+$9, $5+$6}' /proc/stat
}

# One half: flip, wait, then a reading a second. Prints the half's line
# and leaves its mean and median, in watts, in $mean and $median.
half() {
	flip "$2"
	sleep "$settle"
	state=$(asleep)
	before=$(cpu)
	samples=
	i=$settle
	while [ "$i" -lt "$secs" ]; do
		samples="$samples $(cat "$sensor")"
		sleep 1
		i=$((i + 1))
	done
	after=$(cpu)
	# shellcheck disable=SC2086
	set -- $(printf '%s\n' $samples | sort -n | awk '
		{ v[NR] = $1; s += $1 }
		END {
			m = NR % 2 ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2
			printf "%.3f %.3f %.2f %.2f", s / NR / 1e6, m / 1e6, v[1] / 1e6, v[NR] / 1e6
		}')
	mean=$1
	median=$2
	busy=$(echo "$before $after" | awk '{printf "%.1f", 100 * (1 - ($4 - $2) / ($3 - $1))}')
	printf '%3d/%d  %-3s  %s W  (median %s, %s to %s)  CPU %s %%  asleep %s\n' \
		"$count" "$((pairs * 2))" "$half_name" "$mean" "$median" "$3" "$4" "$busy" "$state"
}

echo "Flipping: $parts. $pairs pairs, $((secs - settle)) s counted in each half:" \
	"about $((pairs * 2 * secs / 60)) min. Leave the machine alone."

count=0
pair=0
while [ "$pair" -lt "$pairs" ]; do
	if [ $((pair % 2)) -eq 0 ]; then
		order="off on"
	else
		order="on off"
	fi
	for half_name in $order; do
		count=$((count + 1))
		half "$count" "$half_name"
		if [ "$half_name" = off ]; then
			off_mean=$mean
			off_median=$median
		else
			on_mean=$mean
			on_median=$median
		fi
	done
	echo "$off_mean $on_mean $off_median $on_median" >> "$results"
	pair=$((pair + 1))
done

awk '
	{
		off += $1; on += $2
		d = $1 - $2; sum += d; sq += d * d
		dm += $3 - $4
	}
	END {
		n = NR
		mean = sum / n
		se = n > 1 ? sqrt((sq - n * mean * mean) / (n - 1) / n) : 0
		printf "\noff %.3f W, on %.3f W (means of %d pairs)\n", off / n, on / n, n
		printf "The switches save %.3f W", mean
		if (n > 1)
			printf ", give or take %.3f (standard error)", se
		printf "; by the medians %.3f W.\n", dm / n
		if (n > 1 && (mean < 0 ? -mean : mean) < 2 * se)
			print "That is not told apart from nothing: measure more pairs, or longer halves."
	}' "$results"
