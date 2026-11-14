#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Keep background work on the efficiency cores: the system's daemons
# always, from now and from every boot on, and batch work such as emerge's
# builds while the machine runs from its battery. Run as root:
#   sudo sh ecores/install.sh               set it up (again after this directory changes)
#   sudo sh ecores/install.sh portage       have emerge and what it builds count as batch work
#   sudo sh ecores/install.sh portage-undo  undo portage
#   sudo sh ecores/install.sh remove        undo both, at once
# Nothing needs a reboot or a new login.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
share=/usr/local/share/wmutils/ecores
tool=$share/ecores.sh
start=/etc/local.d/wmutils-ecores.start
rules=/etc/udev/rules.d/60-wmutils-ecores.rules
makeconf=/etc/portage/make.conf
# Our two lines in make.conf: the first tells them apart from the user's.
marker='# wmutils: added by ecores/install.sh portage; emerge and its builds enter the wmutils-batch cgroup.'
setting="PORTAGE_IONICE_COMMAND=\"$tool enter \\\${PID}\""

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 ${1:-}" >&2
	exit 1
fi

die() {
	echo "$*" >&2
	exit 1
}

# udev reads its rules again.
reload() {
	if command -v udevadm >/dev/null && udevadm control --reload 2>/dev/null; then
		echo "udev has read its rules again"
	fi
}

portage_hooked() {
	[ -f "$makeconf" ] && grep -qxF "$marker" "$makeconf"
}

# Takes our two lines, and the empty line before them, out of make.conf.
# Written over the file, not moved onto it: the file keeps its owner, mode
# and label.
portage_undo() {
	portage_hooked || return 0
	tmp=$(mktemp)
	awk -v marker="$marker" '
		$0 == marker { skip = 1; held = 0; next }
		skip { skip = 0; next }
		held { print ""; held = 0 }
		$0 == "" { held = 1; next }
		{ print }
		END { if (held) print "" }' "$makeconf" > "$tmp"
	cat "$tmp" > "$makeconf"
	rm -f "$tmp"
	echo "took PORTAGE_IONICE_COMMAND out of $makeconf"
}

case "${1:-install}" in
install)
	# Without it nothing would move the daemons at boot.
	if [ ! -d "${start%/*}" ]; then
		echo "There is no ${start%/*}, whose *.start scripts s6-rc's init-local" >&2
		echo "or OpenRC's local service runs at boot." >&2
		exit 1
	fi
	# From here first: where it cannot work, nothing is installed.
	sh "$here/ecores.sh" start
	install -D -m 755 "$here/ecores.sh" "$tool"
	echo "installed $tool"
	install -D -m 755 "$here/ecores.start" "$start"
	echo "installed $start"
	install -D -m 644 "$here/ecores.rules" "$rules"
	echo "installed $rules"
	reload
	"$tool" status
	if ! portage_hooked; then
		echo "emerge is not batch work yet: sudo sh $0 portage"
	fi
	;;
portage)
	[ -x "$tool" ] || die "First: sudo sh $0"
	[ -f "$makeconf" ] || die "$makeconf is not a file. Add to Portage's configuration by hand:
$setting"
	if portage_hooked; then
		echo "$makeconf has it already"
	elif grep -Eq '^[[:space:]]*PORTAGE_IONICE_COMMAND=' "$makeconf"; then
		die "$makeconf sets PORTAGE_IONICE_COMMAND already, and it holds one command.
Have that command also run: $tool enter \${PID}"
	else
		printf '\n%s\n%s\n' "$marker" "$setting" >> "$makeconf"
		echo "added PORTAGE_IONICE_COMMAND to $makeconf"
	fi
	echo "From the next emerge on, its builds run on the efficiency cores while on"
	echo "battery, and on every core while the charger is plugged in."
	;;
portage-undo)
	if portage_hooked; then
		portage_undo
	else
		echo "$makeconf has nothing of ours"
	fi
	;;
remove)
	portage_undo
	if [ -x "$tool" ]; then
		"$tool" stop
		echo "the daemons may run on every core again"
	fi
	for f in "$start" "$rules" "$tool"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
	done
	# The directories too, if nothing else is left in them.
	rmdir "$share" "${share%/*}" 2>/dev/null || true
	reload
	;;
*)
	echo "usage: sh $0 [install|portage|portage-undo|remove]" >&2
	exit 2
	;;
esac
