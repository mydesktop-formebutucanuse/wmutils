# selinux

Brings SELinux up on this Gentoo, one step at a time. First permissive:
the kernel loads a policy, s6's init, the services and the logins run in
the policy's domains, and the kernel logs what the policy would refuse and
refuses nothing. Then enforcing, from a point in the boot from which there
is a way back (see [What enforcing covers](#what-enforcing-covers)).

| File | What it is |
|---|---|
| [install.sh](install.sh) | The steps: `status`, `profile`, `packages`, `boot`, `relabel`, `s6`, `policy`, `denials`, `enforcing`, their `-undo`, `remove` |
| [make.defaults](make.defaults) | For the Portage profile: one policy store, `mcs` |
| [dracut.conf](dracut.conf) | Has the initramfs load the policy |
| [selinuxfs.start](selinuxfs.start) | Mounts selinuxfs at every boot, from `/etc/local.d` |
| [rc.selinux](rc.selinux) | Run first by s6-linux-init's `rc.init`: mounts selinuxfs and labels `/run`, `/dev` and Apple's firmware before any service starts |
| [wmutils_s6.te](wmutils_s6.te) | A policy module: what s6 needs and the types borrowed for it do not allow |
| [enforcing.start](enforcing.start) | Switches to enforcing at every boot, from `/etc/local.d`, unless the boot before it went enforcing and nobody got in |

Written for Gentoo's `default/linux/arm64/23.0/desktop` profile,
asahi-kernel 7.2.14, dracut 112 with a UKI, s6-linux-init and s6-rc, and
the SELinux packages of November 2026 (userland 3.11, policy 2.20260616).

**Steps 1 to 6 have run on this machine**, on 2026-11-10 and 11: it boots
with SELinux on, permissive, with the policy loaded and every file
labelled. The world update ran after the reboot here, not before `boot`;
both orders work. `remove` has only run on a copy of `/etc`.

**Step 7, the one for s6, ran on 2026-11-16** and has been through two
boots: init, the services and the logins run in the policy's domains.

**Steps 8 and 9, the policy module and enforcing, have not run on it
yet.** They were tried without root; each step says how.

## How it was

- The kernel is built with SELinux (`CONFIG_SECURITY_SELINUX=y`) but does
  not turn it on: `CONFIG_LSM` is `landlock,yama,bpf`.
- No SELinux tools, libraries or policy are installed.
- The Portage profile has `USE=selinux` masked.
- Neither s6-linux-init nor s6-rc loads a policy or mounts selinuxfs, which
  OpenRC's and systemd's init do themselves.

## The steps

```sh
sh selinux/install.sh
```

says where things stand and what comes next, without root. Each step
stops with a message if the one before it is not done.

### 1. `sudo sh selinux/install.sh profile`

Makes `/etc/portage/make.profile` a directory instead of a link, with

- `parent`: the profile the link pointed to, then Gentoo's
  `features/selinux`. That one unmasks and sets `USE=selinux` (and forces
  `audit` and `caps`), and turns on Portage's own SELinux features.
  Gentoo's ready-made SELinux profile for arm64 is `hardened/selinux`,
  which would take the desktop profile's settings away.
- `make.defaults`: one policy store, `mcs`, instead of all four.
- `wmutils.orig`: the link it replaced, for `remove`.

`eselect profile` no longer manages the profile after this. Portage says
"SELinux module not found" until the next step is done.

### 2. `sudo sh selinux/install.sh packages`

As Gentoo's SELinux installation guide has it:

- `FEATURES="-selinux" emerge --oneshot sec-policy/selinux-base`, which
  brings the tools (libselinux, policycoreutils, checkpolicy, setools,
  audit) with it;
- `/etc/selinux/config`: `SELINUX=permissive`, `SELINUXTYPE=mcs`;
- `selinux-base-policy`, `selinux-policykit` and `selinux-dbus`, with
  Portage's SELinux features off. Without the last two the policy has no
  `file_contexts` and nothing can be labelled (Gentoo bug 891963).

`mcs` is Gentoo's default store. With selinux-base's `unconfined` flag,
which is on by default, users and everything without a policy of its own
run unconfined, as in `targeted`.

### 3. By hand: the world update

```sh
sudo emerge --ask --verbose --update --deep --newuse @world && sudo dispatch-conf
```

On 2026-11-11 that was 67 packages: 24 more policy modules and 43 rebuilds
for the `selinux` flag: glibc, coreutils, util-linux, pam, shadow, elogind,
dbus, polkit, sudo, openssh, Portage, dracut, NetworkManager, PipeWire,
xlibre-server, QEMU and LibreWolf among them. It took an hour on the M1
Pro. Nothing changes at run time yet: the kernel still runs without
SELinux.

It can also run after the reboot of step 5, as it did here, but before
`relabel`. Until coreutils is rebuilt `ls -Z` shows no labels, and `status`
and `relabel` say so. With SELinux on, Portage says `!!! Failed to set new
SELinux execution context` in every phase of every package and builds
them all the same (step 7 says why).

### 4. `sudo sh selinux/install.sh boot`

- Adds `lsm=landlock,yama,selinux,bpf enforcing=0` to `/etc/kernel/cmdline`,
  the command line installkernel builds into the UKI. `lsm=` is the
  kernel's own list with selinux added. `enforcing=0` keeps SELinux
  permissive whatever `/etc/selinux/config` says, and lets the boot go on
  if the policy does not load.
- Installs `/etc/dracut.conf.d/wmutils-selinux.conf`, which adds
  dracut's `selinux` module to the initramfs: it loads the policy from the
  root filesystem before the real init starts. Without a policy the
  kernel's SELinux allows everything and logs nothing.
- Installs `/etc/local.d/wmutils-selinuxfs.start`, which mounts
  selinuxfs at every boot (s6-rc's `init-local` runs it). The initramfs
  unmounts its own, and without it the tools take SELinux for off.

It stops if `/etc/selinux/config` does not say `permissive`, and if the
command line has another `lsm=` or `enforcing=` already.

### 5. By hand: the initramfs, the UKI and a reboot

```sh
sudo emerge --config sys-kernel/asahi-kernel
```

This replaces `EFI/BOOT/BOOTAA64.EFI`, the only thing U-Boot boots; there
is no menu to pick the old one from. installkernel keeps the previous UKI
as `EFI/Linux/*-old.efi` on the ESP. Nothing here stops a boot
(`enforcing=0`, and a policy that does not load is skipped), and it did
boot.

After the reboot, `sh selinux/install.sh` shows `on now: yes; policy
loaded: yes (permissive)`. The files have no labels yet: the kernel takes
every one for `unlabeled_t`.

### 6. `sudo sh selinux/install.sh relabel`

Labels every file, as the guide has it: first the directories that mounts
cover (`/dev`, `/proc`, `/run`, `/sys`, `/home`, `/boot/efi`, ...), each
through a second mount of the filesystem it is on; the swap file as
`swapfile_t`; then `restorecon -RF /`. Both Btrfs subvolumes are labelled; the vfat ESP
cannot hold labels and is skipped.

- A user's FUSE mounts (rclone, the document portal in
  `/run/user/1000/doc`) are closed to root. restorecon says `Could not
  stat ...: Permission denied` for each, labels everything else and fails
  all the same. The step lets that message pass for the FUSE mounts root
  cannot enter, names them at the end, and stops on any other error.
- Files in `/tmp` and `/var/tmp` keep what they had: the policy gives
  them no label (`<<none>>`). `/tmp` is emptied at the next boot.
- It stops before it starts if `ls -Z` shows no labels, which is a
  coreutils from before the world update.

On 2026-11-11 no file outside `/tmp` and `/var/tmp` was `unlabeled_t`
afterwards, out of about a million.

### 7. `sudo sh selinux/install.sh s6`

After step 6 the policy is loaded and confines nothing. What a boot with
every file labelled showed on 2026-11-11:

- s6-linux-init and everything s6-rc starts have no types in the policy:
  the init script is `etc_t` and `s6-svscan` is `bin_t`, so nothing takes
  the init out of the kernel's own domain. The init, every service, the
  gettys, the logins and the X session ran as
  `system_u:system_r:kernel_t:s0`, which is unconfined; only udev had a
  type of its own.
- Logins got no context. pam_selinux is in `/etc/pam.d/system-login`, but
  the policy has default contexts for a login from `local_login_t`,
  `sshd_t`, `xdm_t` or `init_t`, and none for one from `kernel_t`. `id -Z`
  said `kernel_t` too, and Portage cannot switch to its own context from
  there, which is the message of step 3.
- `/run`, which s6-linux-init mounts, was `tmpfs_t`, and every device node
  `device_t`: the initramfs relabels `/dev` only after it has unmounted
  selinuxfs, which does nothing, and udev started before the script of
  step 4 mounts selinuxfs again, so it took SELinux for off.
- The boot and a few minutes of use left 13 `avc:` lines in the kernel
  log.

The policy has no types for s6. This step gives s6's files the types the
policy has for the init and the init scripts it knows, as local file
contexts (`semanage fcontext`), which a later `relabel` and Portage keep:

| File | Type | What the policy does with it |
|---|---|---|
| `/etc/s6-linux-init/current/bin/init`, where `/sbin/init` leads | `init_exec_t` | The kernel's domain becomes `init_t`: s6-linux-init, then s6-svscan as process 1 |
| `/usr/bin/s6-supervise` and `/etc/s6-linux-init/current/scripts/*` | `initrc_exec_t` | `init_t` becomes `initrc_t`, the domain of init scripts: the supervisors and the run scripts, `rc.init`, s6-rc. From there the policy has a domain for every daemon it knows (dbus, NetworkManager, udev, chrony, elogind, bluetooth, ...) and for getty; from getty, login is `local_login_t`, and pam_selinux gives the user `unconfined_t` |
| `/run/service` and `/run/uncaught-logs` | `initrc_runtime_t` | What `initrc_t` makes in `/run` anyway; s6-linux-init copies these there before anything can label them |

What has no policy of its own (`s6-log`, rclone, speakersafetyd) stays in
`initrc_t`, which is unconfined, like the user's `unconfined_t`.

It also installs [rc.selinux](rc.selinux) as
`/etc/s6-linux-init/current/scripts/wmutils-selinux` and adds one line to
`rc.init`, as its second, that runs it before s6-rc starts anything:

- It mounts selinuxfs. udev and the daemons built with SELinux label what
  they make only if they find it when they start. The script of step 4
  stays; it finds selinuxfs mounted and does nothing.
- `restorecon -RF /run /dev`: `/run` becomes `var_run_t`, from which the
  policy gives what each service makes there its type, and the device
  nodes from before udev get theirs (`tty_device_t`,
  `fixed_disk_device_t`, `sound_device_t`, ...).

The line cannot stop `rc.init`, and the script always ends well; on a boot
without SELinux or without a policy it does nothing.

SELinux stays permissive, so nothing is refused that was allowed before;
what changes is which domain the log names, and that the log now says what
the policy would refuse the services and the logins. Nothing changes until
the next boot. After it:

```sh
sh selinux/install.sh
ps -eo label,args
id -Z
```

`status` says which domain the init runs in, `ps` the domain of every
process, and `id -Z` should say `unconfined_u:unconfined_r:unconfined_t`.

If the boot does not get as far as a login: s6-linux-init's early getty on
tty6 does not depend on `rc.init`, and from there

```sh
sudo sh selinux/install.sh s6-undo
```

takes the line, the script and the file contexts away again and puts the
old labels back.

A new base directory from `s6-linux-init-maker` has neither the line nor
the script; `status` says so, and the step can run again.

What was tried, without root, on 2026-11-11:

- `s6`, `s6` again and `s6-undo` on copies of `/etc/s6-linux-init` and
  `/etc/selinux` in a user namespace, with a stand-in for `semanage` and
  the real `restorecon`: the labels as in the table, `rc.init` different
  by its second line only, and after the undo the same as before, byte
  for byte. `remove` after `s6`, on a copy of `/etc`, the same way.
- `restorecon -n` over the real `/run` and `/dev` with those contexts.
- The boot script with stand-ins for `mount` and `restorecon`, with
  selinuxfs there, missing, and failing to mount; and the line in
  `rc.init` with the script missing, failing and not executable.
- The domains, with a private `s6-svscan` over copies that carry the
  labels: `init_t` from the init script, `initrc_t` for s6-supervise and
  the run scripts, then `getty_t` and `local_login_t`, and
  `system_dbusd_t`, `NetworkManager_t`, `udev_t`, `chronyd_t`,
  `syslogd_t`, `bluetooth_t` and `systemd_logind_t` for copies labelled
  as those daemons are. From `local_login_t` the policy's default context
  for the user is `unconfined_t`.

On 2026-11-16 the step ran as root, with the real `semanage`, and the
machine booted with it, twice. What `ps -eo label,args` showed:

- process 1 (`s6-svscan`) is `init_t`, and only kernel threads are left in
  `kernel_t`;
- `s6-supervise`, `s6-log`, s6-rc's own services, `speakersafetyd` and
  `rclone` are `initrc_t`;
- `NetworkManager_t`, `system_dbusd_t`, `systemd_logind_t` (elogind),
  `chronyd_t`, `bluetooth_t`, `policykit_t`, `rtkit_daemon_t`, `syslogd_t`
  (metalog), `udev_t`, `getty_t` and `local_login_t`;
- the login shell, the X session and everything in it
  `unconfined_u:unconfined_r:unconfined_t`, the X server `xserver_t`;
- `/run` is `var_run_t`, `/run/service` `initrc_runtime_t`, and the device
  nodes have their types.

`s6-undo` ran once too, between the two boots, and `s6` again after it.

### 8. `sudo sh selinux/install.sh policy`

With the services in their domains the kernel log says what the policy
would refuse them. A permissive boot and a login on 2026-11-16, with the
log's rate limit off (see below), left 127 lines, 67 different things.
The script of step 9 switches about two seconds into the boot, so most
of them would have been refused. What they were:

- **What everything s6 starts inherits.** The catch-all logger's fifo,
  `/run/service/s6-svscan-log/fifo`, is the output of process 1 and of
  every service and oneshot without a logger of its own; the policy lets
  a daemon write to a pipe an init script made, not to this fifo. The
  same for `/dev/console` as the kernel opened it, and for the service
  directory a service runs in. Enforcing, the kernel closes such
  descriptors when the program starts.
- **X.** elogind opens the screen and the input devices for the X server
  and sends the descriptors over the system bus, and the bus daemon may
  not pass on a DRM or an input device.
- **Firmware.** On Apple hardware the initramfs copies the vendor
  firmware from the ESP to a tmpfs on `/usr/lib/firmware/vendor` before
  any policy is loaded, so its files are `root_t`, and the kernel may
  load firmware from `lib_t`. Wi-Fi and Bluetooth load theirs again when
  they are turned on.
- **`/run/NetworkManager` and `/run/tmpfiles.d`**, which s6-rc's scripts
  make before the daemon runs. The policy gives a directory in `/run` the
  daemon's type when the daemon makes it, and for some names when an init
  script does (`dbus`, `chrony`, `udev`); not for these two, which came
  out `initrc_runtime_t`, where NetworkManager may not write.
- **elogind**, in the domain the policy has for systemd-logind: watching
  a session's cgroup, `/proc/pressure`, the boot entries on the ESP, the
  scheduling of a login; and rtkit asking it to hold a suspend back.
- **udev**: elogind's `uaccess`, which gives the user at the seat the
  sound, video and input devices, reading the seat; a rule that gives
  the backlight's `brightness` to a group.
- **rclone's FUSE mount**, from a service that runs as the user:
  fusermount3 hands the descriptor back over a socket the service made,
  and the directory under the mount had no label at all (`relabel` only
  got to mount points on `/` before 2026-11-16).
- **xauth**, which startx has write the server's key to a file in `/tmp`.
- **`io_uring_setup`.** This kernel asks the policy for a permission of
  its own, `io_uring allowed`. The policy has the permission and gives it
  to no domain, the unconfined ones included.
- **Memory that is written and then run**, which every JIT compiler
  needs: Electron programs, the browser. The policy has a boolean for it,
  `allow_execmem`, off by default.

The step builds [wmutils_s6.te](wmutils_s6.te) with `checkmodule` and
`semodule_package`, loads it with `semodule -i` (which builds the whole
policy again and has the kernel load it), and turns `allow_execmem` on
for good (`setsebool -P`). It relabels the two directories in `/run`,
the firmware, and the directories under mount points on other
filesystems than `/`. It also puts [rc.selinux](rc.selinux), which now
labels the firmware at every boot, where step 7 put it. Policy updates
from Portage keep the module and the boolean.

Each rule in the module is for a line of that log, and says which. Two
are wider than the line: every domain may write to the catch-all fifo
and use the console descriptor, not only the ones the log named.

`allow_execmem` lets every unconfined program run memory it wrote. The
desktop needs that; it is also one of the protections SELinux has
against injected code, and the unconfined domains are without it after
this.

Twelve of the 67 stay in the log of a boot, all from before the switch:
s6-linux-init's early getty on tty6 and `restorecon` itself, which start
while `/dev` and `/run` are being labelled (8); the kernel loading a
module from the initramfs and the firmware before it is labelled (2);
and two that go once the step has run (the mount on the unlabelled
directory, and NetworkManager asking for `CAP_PERFMON`, which it does
without and the module tells the kernel not to log).

Tried without root on 2026-11-16: the module built, linked with the 68
policy packages in `/usr/share/selinux/mcs` and expanded to a policy, and
`sesearch` on that policy for each of the 67: 55 allowed, the twelve
above not. The first version of the module (the fifo for daemons, the
two directories, io_uring) went through `semodule -i` and a boot. Not
tried: this version on the real policy store, and a boot with it.

`policy-undo` takes the module out and turns the boolean off.

### Looking at what it would refuse

```sh
sudo sh selinux/install.sh denials
```

No auditd runs under s6, so the denials go to the kernel log, which is
root's to read. The step prints one line for each thing refused: how
often, the domain, the type and class it was refused on, what, the
command, and the file if the kernel named one. `denials raw` prints the
lines as the kernel logged them, for `audit2allow -i`.

The kernel prints audit records through a rate limit, ten lines in five
seconds, and a boot has hundreds: on 2026-11-16 the record numbers went
from 10 to 177 between two lines. [rc.selinux](rc.selinux) takes the
limit off (`kernel.printk_ratelimit=0`) at the start of a boot that is
permissive, and the script of step 9 puts it back when it switches.

In permissive mode the kernel logs each thing once for a domain: it
remembers that it let it pass. So the log of a boot is the list, and it
has only what was done since: before enforcing, do what the machine does every
day (suspend and resume, a network that comes and goes, Bluetooth, sound,
`emerge`, `emerge --config sys-kernel/asahi-kernel`) and read it again.

### 9. `sudo sh selinux/install.sh enforcing`

First, at run time, where a reboot undoes it:

```sh
sudo setenforce 1
sudo setenforce 0
```

and between the two: `sudo` itself, a login on another console, and
whatever `denials` now says was refused.

The step then makes it the state after every boot. It installs
[enforcing.start](enforcing.start) as
`/etc/local.d/wmutils-selinux-enforcing.start` and makes
`/etc/selinux/wmutils-enforcing`, the file that tells it to switch. It
does not switch now. It stops unless step 7 is through a boot (`init_t`)
and step 8 is done.

At boot the script

1. does nothing if SELinux is not on and permissive with a policy loaded;
2. finds `/etc/selinux/wmutils-enforcing.trying`, its own mark from the
   boot before, and then leaves SELinux permissive and says so;
3. otherwise writes the mark, switches (`/sys/fs/selinux/enforce`), puts
   the log's rate limit back, and waits in the background for a user to
   be logged in, and still logged in 30 seconds later: a process of a uid
   from 1000 on, outside `initrc_t`. Then it removes the mark.

So a boot that enforcing breaks before anybody is logged in is followed,
after the power button, by one that is permissive, and so is every boot
after it until the step runs again. `sh selinux/install.sh` says which
of the two happened.

`enforcing-undo` removes the script, the file and the mark, and switches
back to permissive at once. `s6-undo` and `policy-undo` do that first.

Tried without root on 2026-11-16, on copies of the paths: not armed;
armed with nobody logged in (the mark stays, and the next run does not
switch); armed with a user logged in (the mark goes after the wait).
Not tried: the step as root, and a boot.

## What enforcing covers

- **From `init-local` on, not from the start of the boot.** The kernel
  command line still says `enforcing=0`, the initramfs, s6-linux-init and
  s6-rc's first services run permissive, and the switch comes when the
  root filesystem is writable, beside the daemons and the gettys. The
  mark has to be written before the switch, and the command line is
  inside the UKI, which U-Boot boots without a menu: enforcing from the
  first instruction would have no way back but another system on a USB
  stick. `/etc/selinux/config` keeps saying `permissive`, which is what
  the boot starts as.
- **The daemons the policy has a domain for**: NetworkManager, D-Bus,
  elogind, chrony, Bluetooth, polkit, rtkit, metalog, udev, the gettys
  and login. A domain is as tight as Gentoo's policy for OpenRC makes it.
- **Not the user's session, and not what the policy does not know.**
  `unconfined_t` (the login, X, everything started from it) and
  `initrc_t` (`s6-supervise`, `s6-log`, `speakersafetyd`, `rclone`) may do
  what they could before, except what `allow_execmem` and the other
  booleans govern.
- **A login that worked before the switch proves little.** With an
  automatic login the user may be in before `init-local` gets to the
  script. The mark then goes although no login happened under enforcing;
  what stays tested by the boot is that the session lives through the
  switch. `sudo` and a second login are for the run-time trial above.

## Undoing it

```sh
sudo sh selinux/install.sh remove
```

undoes steps 9, 8 and 7 as `enforcing-undo`, `policy-undo` and `s6-undo`
do, puts the profile link back, takes `lsm=`
and `enforcing=0` out of `/etc/kernel/cmdline`, and removes the dracut
drop-in and the selinuxfs script. Then
`emerge --config sys-kernel/asahi-kernel` and a reboot for a kernel
without SELinux, `emerge --ask --update --deep --newuse @world` to build
the packages without the flag again, and `emerge --ask --depclean` for the
tools and policies. The labels stay on the files, where nothing reads
them.
