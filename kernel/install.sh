#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Set up Gentoo so that every sys-kernel/asahi-kernel build applies the
# patches in this directory, each one if it fits, and only warns about one
# that does not; leaves out the drivers in trim.config; turns on the idle
# power options in power.config; and stops before compiling if the config
# lacks an option in required.txt. Also set appledrm's options (VRR, notch),
# and let the SD card reader, the HDMI converter and the USB-C ports sleep
# from boot while nothing is plugged in, and the I2C buses between
# transfers (unplugged.conf, unplugged.rules).
# Run as root:
#   sudo sh kernel/install.sh          set it up (again after this directory changes)
#   sudo sh kernel/install.sh remove   undo it
# The next kernel build (an update, or `emerge --oneshot sys-kernel/asahi-kernel`)
# picks it up.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
share=/usr/local/share/wmutils
hook=/etc/portage/env/sys-kernel/asahi-kernel
plain=/etc/portage/patches/sys-kernel/asahi-kernel
modconf=/etc/modprobe.d/wmutils-appledrm.conf
unplugconf=/etc/modprobe.d/wmutils-unplugged.conf
unplugrules=/etc/udev/rules.d/60-wmutils-unplugged.rules
trim=/etc/kernel/config.d/wmutils-trim.config
power=/etc/kernel/config.d/wmutils-power.config
# The project was utils-mydesktop until November 2026: what it installed
# under that name goes once the same is there under the new one.
oldshare=/usr/local/share/utils-mydesktop
oldmodconf=/etc/modprobe.d/utils-mydesktop-appledrm.conf
oldtrim=/etc/kernel/config.d/utils-mydesktop-trim.config

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 ${1:-}" >&2
	exit 1
fi

# A file belongs to us only if it starts with our marker, or with the one
# of the old name.
marked() {
	[ -f "$1" ] && head -n 1 "$1" | grep -Eq '^# (wmutils|utils-mydesktop): '
}
ours() {
	marked "$hook"
}

# Removes what was installed under the old name.
forget_old() {
	for f in "$oldshare"/*.patch "$oldshare/required.txt"; do
		[ -e "$f" ] || continue
		rm -f "$f"
		echo "removed $f (the old name)"
	done
	rmdir "$oldshare" 2>/dev/null || true
	for f in "$oldtrim" "$oldmodconf"; do
		if marked "$f"; then
			rm -f "$f"
			echo "removed $f (the old name)"
		fi
	done
}

case "${1:-install}" in
install)
	if [ -e "$hook" ] && ! ours; then
		echo "$hook exists and is not ours; add the contents of" >&2
		echo "$here/asahi-kernel.bashrc to it by hand." >&2
		exit 1
	fi
	mkdir -p "$share"
	# Patches that left the series go too.
	for p in "$share"/*.patch; do
		[ -e "$p" ] && [ ! -e "$here/${p##*/}" ] || continue
		rm -f "$p"
		echo "removed $p"
	done
	for p in "$here"/*.patch; do
		name=${p##*/}
		install -m 644 "$p" "$share/$name"
		echo "installed $share/$name"
		# The hook replaces a plain user patch, which would stop the
		# build instead of warning when it no longer fits.
		if [ -e "$plain/$name" ]; then
			rm -f "$plain/$name"
			echo "removed $plain/$name (the hook applies it now)"
		fi
	done
	rmdir "$plain" "$(dirname "$plain")" 2>/dev/null || true
	install -m 644 "$here/required.txt" "$share/required.txt"
	echo "installed $share/required.txt"
	install -D -m 644 "$here/asahi-kernel.bashrc" "$hook"
	echo "installed $hook"
	# kernel-build.eclass merges /etc/kernel/config.d/*.config last.
	install -D -m 644 "$here/trim.config" "$trim"
	echo "installed $trim"
	install -D -m 644 "$here/power.config" "$power"
	echo "installed $power"
	# The kernel build puts it in the initramfs, where appledrm loads.
	install -D -m 644 "$here/appledrm.conf" "$modconf"
	echo "installed $modconf"
	# Both go into the initramfs too: appledrm and the USB-C drivers
	# load there. Until the next kernel build has made a new one, the
	# HDMI converter's option is not read at boot.
	install -D -m 644 "$here/unplugged.conf" "$unplugconf"
	echo "installed $unplugconf"
	install -D -m 644 "$here/unplugged.rules" "$unplugrules"
	echo "installed $unplugrules"
	forget_old
	echo "The next asahi-kernel build applies the patches (or warns about any that no longer fit),"
	echo "leaves out the drivers in trim.config, turns on the options in power.config,"
	echo "and checks the config against required.txt."
	echo "From the next boot the SD card reader and the USB-C ports sleep while nothing"
	echo "is plugged in, and the I2C buses between transfers; from the next kernel"
	echo "build the HDMI converter too, and the HDMI port's PHY and crossbar with it."
	;;
remove)
	forget_old
	if ours; then
		rm -f "$hook"
		rmdir "$(dirname "$hook")" 2>/dev/null || true
		echo "removed $hook"
	fi
	rm -f "$share"/*.patch "$share/required.txt"
	rmdir "$share" 2>/dev/null || true
	echo "removed the patches and required.txt from $share"
	for f in "$trim" "$power" "$modconf" "$unplugconf" "$unplugrules"; do
		if marked "$f"; then
			rm -f "$f"
			echo "removed $f"
		fi
	done
	rmdir "$(dirname "$trim")" 2>/dev/null || true
	echo "The next asahi-kernel build is unpatched and has the stock config."
	echo "From the next boot nothing is told to sleep while unplugged."
	;;
*)
	echo "usage: sh $0 [install|remove]" >&2
	exit 2
	;;
esac
