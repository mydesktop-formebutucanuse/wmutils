#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs audweb for the user running it, without root: audweb and
# audweb-host in ~/.local/bin, the native messaging manifest that lets the
# LibreWolf extension start audweb-host, and the extension itself as
# ~/.local/share/audweb/audweb.xpi, to be installed in LibreWolf by hand
# (see README.md).
#   sh audweb/install.sh          install or update
#   sh audweb/install.sh remove   undo it (the extension stays in LibreWolf)
# The Gentoo package installs the same for every user; this is for running
# it from the source tree, or without the package.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin=$HOME/.local/bin
share=$HOME/.local/share/audweb
# LibreWolf reads native messaging manifests from Firefox's directory too,
# besides its own (~/.librewolf/native-messaging-hosts).
hosts=$HOME/.mozilla/native-messaging-hosts

case ${1:-install} in
install)
	mkdir -p "$bin" "$share" "$hosts"
	install -m 755 "$here/audweb" "$here/audweb-host" "$bin/"
	sed "s|@HOST@|$bin/audweb-host|" "$here/audweb.json.in" >"$hosts/audweb.json"
	rm -f "$share/audweb.xpi"
	(cd "$here/librewolf" && zip -q -X "$share/audweb.xpi" manifest.json background.js icon.svg)
	echo "Installed. Now add $share/audweb.xpi to LibreWolf: see $here/README.md."
	;;
remove)
	rm -f "$bin/audweb" "$bin/audweb-host" "$hosts/audweb.json" "$share/audweb.xpi"
	rmdir "$share" 2>/dev/null || :
	echo "Removed. Take the extension out in LibreWolf's about:addons."
	;;
*)
	echo "usage: sh $0 [install|remove]" >&2
	exit 2
	;;
esac
