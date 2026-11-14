# kernel

Patches for the Asahi Linux kernel: the panel backlight without visible
steps, the PMP booted for lower idle power, the ProMotion display's
variable refresh rate (VRR) on, cores powered down for shorter idle
periods, a Wi-Fi radio that sleeps sooner, and the SD card reader, the
HDMI converter, the HDMI port's PHY and the USB-C ports powered down
while nothing is plugged in, the I2C buses between transfers, and a switch
that reads the keyboard and trackpad without interrupts. Plus appledrm options: VRR, and the full
panel height around the notch. And a smaller kernel config, without the
drivers this MacBook cannot use, so that a kernel build takes less time,
with three options on for fewer wakeups while the machine is idle.

| File | What it is |
|---|---|
| [0001-drm-apple-backlight-1-64-nit.patch](0001-drm-apple-backlight-1-64-nit.patch) | The panel backlight counts in 1/64 nit instead of whole nits |
| [0002-drm-apple-backlight-without-blocking.patch](0002-drm-apple-backlight-without-blocking.patch) | A brightness write returns at once, and the brightness can change about 60 times a second instead of 21 |
| [0003-arm64-dts-apple-j314s-boot-the-pmp.patch](0003-arm64-dts-apple-j314s-boot-the-pmp.patch) | Boots the Power Management Processor on the 14-inch M1 Pro (J314s) |
| [0004-drm-apple-adaptive-sync-promotion.patch](0004-drm-apple-adaptive-sync-promotion.patch) | VRR for the ProMotion panel and Adaptive Sync monitors, behind `appledrm.force_vrr` |
| [0005-cpuidle-apple-power-down-for-shorter-idle.patch](0005-cpuidle-apple-power-down-for-shorter-idle.patch) | A core is powered down for idle periods from 2 ms instead of 10 ms; adjustable while running |
| [0006-wifi-brcmfmac-power-save-timeout.patch](0006-wifi-brcmfmac-power-save-timeout.patch) | In power save the Wi-Fi radio stays awake 200 ms after traffic instead of 2 s; adjustable while running |
| [0007-mmc-sdhci-pci-gli-gl9755-runtime-pm.patch](0007-mmc-sdhci-pci-gli-gl9755-runtime-pm.patch) | The SD card reader can sleep while its slot is empty. Off until `sdhci_pci.gl9755_runtime_pm=1`, which unplugged.conf sets |
| [0008-drm-apple-hdmi-converter-off-unplugged.patch](0008-drm-apple-hdmi-converter-off-unplugged.patch) | The HDMI port's converter chip can be switched off while no monitor is plugged in. Off until `appledrm.hdmi_idle_off=1`, which unplugged.conf sets |
| [0009-usb-apple-typec-runtime-pm.patch](0009-usb-apple-typec-runtime-pm.patch) | A USB-C port's controller and PHY can be powered down while nothing is plugged into it. Off until the port's `power/control` is `auto`, which unplugged.rules makes it |
| [0010-drm-apple-hdmi-phy-off-unplugged.patch](0010-drm-apple-hdmi-phy-off-unplugged.patch) | The HDMI port's PHY and the display crossbar in front of it can be released, and powered down, along with the converter. Off until `appledrm.hdmi_idle_phy_off=1`, which unplugged.conf sets |
| [0011-i2c-apple-runtime-pm.patch](0011-i2c-apple-runtime-pm.patch) | An I2C bus's controller can be powered down between transfers. Off until the bus's `power/control` is `auto`, which unplugged.rules makes it |
| [0012-spi-apple-poll-threshold.patch](0012-spi-apple-poll-threshold.patch) | The SPI driver's polling threshold becomes a parameter, `spi_apple.poll_us`; at 8 the keyboard and trackpad are polled instead of read by sixteen interrupts a packet. At its default (5) nothing changes |
| [appledrm.conf](appledrm.conf) | Sets `appledrm.force_vrr=1` and `appledrm.show_notch=1` |
| [unplugged.conf](unplugged.conf) | Module options that turn 0007, 0008 and 0010 on from boot |
| [unplugged.rules](unplugged.rules) | udev rules that turn 0009 and 0011 on from boot, for the three USB-C ports and the four I2C buses, and allow the HDMI port's PHY and crossbar to sleep once 0010 releases them |
| [unplugged-ab.sh](unplugged-ab.sh) | Measures what those save: flips them off and on in turns and compares the power draw |
| [trackpad-ab.sh](trackpad-ab.sh) | Measures 0012 with a finger on the trackpad: interrupts and polling in turns, with the interrupts, wakeups and power draw of each |
| [trim.config](trim.config) | A config fragment that turns off drivers for hardware this MacBook cannot take, kernel self-tests, and server parts |
| [power.config](power.config) | A config fragment that turns on three options for fewer wakeups at idle: power-efficient workqueues, RCU callbacks in kernel threads, lazy RCU |
| [required.txt](required.txt) | The options this Mac's own drivers need; a build stops before compiling if one is off |
| [install.sh](install.sh) | Makes every Gentoo `asahi-kernel` build apply the patches, trim.config and power.config and check required.txt, and installs appledrm.conf, unplugged.conf and unplugged.rules |
| [asahi-kernel.bashrc](asahi-kernel.bashrc) | The Portage hook that install.sh puts in place |

Each patch works without the others (0008 changes `dcp.c`, as 0004 does,
in other places), except 0010: it adds to what 0008 put in `dcp.c`, and
the PHY sleeps only with 0009. The backlight patches change only
`drivers/gpu/drm/apple/dcp_backlight.c`, which serves the built-in panel
only: external displays have no backlight device and are not affected.

Written against `asahi-7.2.14-1` (Gentoo `sys-kernel/asahi-kernel-7.2.14_p1`);
0005 to 0012 against `asahi-7.3.3-2` (`asahi-kernel-7.3.3_p2`).

## 1/64-nit backlight

`apple-panel-bl` counts in whole nits, and a 1-nit step is easy to see at
low brightness: 20 % at 5 nits, 2 % at 50, 0.8 % at 125. Slow brightness
changes, such as [autobrightd](../autobrightd)'s, then move in visible
steps.

The driver turns nits into a panel DAC value by linear interpolation in two
lookup tables, so finer input costs nothing. With the patch the device
counts in 1/64 nit (`max_brightness` 500 becomes 32000 on a 14-inch MacBook
Pro). Whole nits give exactly the same DAC values as before, and the 63
values between two nits fall on the same lines. This was checked for every
value from 0 to 32576 against the driver's own arithmetic: whole nits match
the old DAC bit for bit, and the DAC rises with every step from 2 nits up.

1/64 rather than 1/16 because of the second patch: at 60 updates a second a
slow change moves well under 1/16 nit per update in the dark, and would go
in uneven 1/16-nit jumps (3 % at 2 nits).

Tools that scale by `max_brightness` (logind, brightnessctl, desktop
sliders, [backlight](../backlight), autobrightd) work as before. A script
that writes raw nits now gets 1/64 of them.

## Brightness writes without blocking

Every write to the backlight made the driver sleep 42 ms (one frame at
23.976 Hz, in the hope that a display update carries the new brightness
meanwhile) and then run a display commit, all inside the write and under
the backlight lock. A write took 43 to 47 ms on an M1 Pro, so the
brightness could change at most about 21 times a second.

With the patch the write returns at once, and the driver's existing work
item applies it. It waits one frame at 120 Hz (8.4 ms) instead of 42 ms:

| | Before | After |
|---|---|---|
| A write | 43 to 47 ms | returns at once |
| Brightness updates, screen changing | about 21 a second | up to 120 a second: the next frame carries the brightness, no extra commit |
| Brightness updates, screen still | about 21 a second | about 60 a second: wait a frame, then commit |
| Writes faster than the display | wait their turn | merged; the latest value is shown |

On suspend the update is still finished before the backlight core goes on,
so it happens before the display suspends, as before. The writer no longer
sees errors from the display commit.

Costs, measured on an M1 Pro before the patch: each write costs about
0.5 ms of CPU, most of it udev handling the uevent the backlight core sends
for every write. At 60 writes a second that is about 3 % of one core while
the brightness is changing, and nothing while it is not.

## PMP

The Power Management Processor (PMP) is a coprocessor that takes power
reports from the rest of the SoC and powers down idle parts of it. The
Asahi kernel builds its driver (`pmp.ko`) but leaves the device off in the
devicetree until the driver is upstream. The [Asahi Linux 7.0 progress
report](https://asahilinux.org/2026/04/progress-report-7-0/) measured about
half a watt, some 20 %, less at idle on a 14-inch M1 Pro with it on, and
says how to opt in: define `APPLE_USE_PMP` in the board's DTS.

The patch does that for `t6000-j314s.dts` (14-inch M1 Pro, the only board
it changes) and enables the `pmp` node. `APPLE_USE_PMP` routes the power
domains of the display controllers (DCP, DCPext), the camera ISP and the
video decoder through the PMP's report entries. The compiled DTB differs
from the stock one only there and in the node's status.

After booting a kernel with it, `lsmod | grep '^pmp'` lists the driver and
`/sys/firmware/devicetree/base/soc/pmp@28e700000/status` reads `okay`. If
it misbehaves (display, camera or video decoding stop powering up, or
`dmesg` shows `PMP firmware crashed`), boot with `modprobe.blacklist=pmp`
and remove the patch.

## VRR (ProMotion, Adaptive Sync)

A backport of James Calligeros's [Adaptive Sync/ProMotion
work](https://github.com/AsahiLinux/linux/pull/477) (seven commits on
`chadmed/linux` `dcp/vrr`, not yet merged into the Asahi kernel), adjusted
in one place for 7.2.14. DCP does VRR itself: the driver sets a minimum
refresh rate in the adaptive sync parameter and sets the mode again, and the
refresh rate then follows the frames' swap timestamps. The built-in panel
of the 14- and 16-inch MacBook Pros runs anywhere from 24 to 120 Hz;
external monitors use the range in their EDID.

It is off unless `appledrm.force_vrr=1`, which [appledrm.conf](appledrm.conf)
sets. KMS and the compositor are not told (`vrr_capable` stays 0), because
KMS does not allow the mode set DCP needs to switch it; the X server and
Window Maker need nothing. Upstream calls it experimental and reports that
mpv's `gpu` and `gpu-next` outputs can halt the GPU in fullscreen on the
built-in display. To turn it off without rebuilding, set `force_vrr=0` in
`/etc/modprobe.d/wmutils-appledrm.conf` and regenerate the
initramfs (`emerge --config sys-kernel/asahi-kernel`), or boot once with
`appledrm.force_vrr=0`.

`cat /sys/module/appledrm/parameters/force_vrr` shows `Y` when it is on.

## The notch

By default appledrm hides the strip on both sides of the camera notch and
gives X a 16:10 screen below it (3024x1890 on the 14-inch MacBook Pro).
`appledrm.show_notch=1`, also in [appledrm.conf](appledrm.conf), gives X
the whole panel instead (3024x1964): the top 74 pixels are then usable, and
the notch covers the middle of them. The driver reads the option only when
it starts, so it needs no patch but takes a reboot with the new initramfs.

Windows do not know about the notch: anything placed at the very top, a
maximised window's title bar for one, goes partly under it. A dock window
that fills the strip and reserves it with a strut
keeps them below it. Window Maker's own icons and clip are not in the
middle of the top edge and are not covered.

`cat /sys/module/appledrm/parameters/show_notch` shows `Y`, and `xrandr`
the taller mode, when it is on.

## A smaller config

The `asahi-kernel` ebuild starts from Fedora's config, which is made for
every arm64 machine Fedora runs on: 4767 options build modules (4946
files) and 2933 are compiled in. Most of them are for hardware a MacBook
cannot have. [trim.config](trim.config) turns those off, which leaves 2018
options building modules (2156 files) and 2565 compiled in. Compiling the
kernel and its modules (`make -j8 all`, as the ebuild does) took 11
minutes on this M1 Pro; with the stock config it had not finished after
17, having compiled half again as many files and linked no module yet.
Those numbers are 7.2.14's. In 7.3.3 the stock config has 4816 and 2960,
and the list leaves 2013 and 2587.

It is a config fragment: the ebuild merges every `*.config` file in
`/etc/kernel/config.d` last, and the kernel's `olddefconfig` then drops
whatever depended on what the fragment turns off. It only turns things
off: nothing is on that is off in the stock config. What goes:

- Whole menus for hardware that cannot be connected to a 14-inch MacBook
  Pro: PCI Ethernet, InfiniBand and RDMA, SATA controllers, PCI sound
  cards and HD Audio, SoundWire and SLIMbus, PCI capture cards, camera
  sensors on boards, FPGAs, CoreSight, raw NAND flash, the staging
  drivers, and the Chromebook, Surface and Mellanox platform drivers. This
  kernel tunnels USB, DisplayPort and networking over Thunderbolt but not
  PCIe, so a PCI chip in a dock or enclosure cannot reach the Mac either.
- TV and radio tuners and analog video capture, USB ones included.
- Kernel self-tests: KUnit, the torture tests, null_blk, netdevsim.
- Cluster and server parts with nothing installed to use them: the SCSI
  target (and with it vhost-scsi), GFS2, DLM, Ceph, DRBD, the NFS server,
  RDMA over Ethernet.
- One by one, drivers for chips that only sit on circuit boards, on I2C,
  SPI or PCI or inside other SoCs: SCSI, SAS, RAID and Fibre Channel
  cards, hardware monitors, regulators and PMICs, real-time clocks, GPIO
  expanders, LED controllers, IIO sensors, touchscreens and keypads, audio
  codecs, video decoders, GPUs, display panels and bridges, watchdogs,
  flash chips, crypto engines, Wi-Fi chips, CAN controllers, TPMs, and
  the calls into the firmware of Qualcomm SoCs.

What stays:

- Every driver this Mac uses. [required.txt](required.txt) lists their
  options: those of every module it loads, and of the driver for every
  device it has.
- Everything that can come in over USB-C or Bluetooth, except the tuners
  and capture devices above: USB Ethernet and Wi-Fi adapters, storage,
  serial adapters, audio interfaces, webcams, DisplayLink and other USB
  displays, keyboards, mice, game controllers and other HID devices, CAN
  adapters, NFC readers.
- Generic software other than the server parts above: file systems,
  network protocols, netfilter, KVM, vhost, virtio (the guest drivers
  too), md and dm, iSCSI over TCP, UBI and UBIFS (with block2mtd and
  mtdram for flash images). Containers, virtual machines, VPNs and disks
  from other machines work as before.
- Mice, tablets, joysticks and CAN adapters with a serial port, which
  `inputattach` or `ldattach` binds to a USB-serial adapter. The serial
  drivers that go bind only to a device named in a board's device tree.

### How it was checked

The list was generated from the kernel's Kconfig files and the stock
config, then merged the way the ebuild merges it, and the result compared
with the stock config:

- The 115 options in required.txt keep their stock values. They come from
  `lsmod` and from the modalias of every device in sysfs, resolved with
  `modprobe -R`.
- No USB or HID driver goes, except the tuners and capture devices above.
  The driver-by-driver part turns off only modules whose Kconfig entry
  sits with the board chips, and none that a kept driver, or a USB or HID
  driver, depends on.
- Each of the 368 options compiled into the stock kernel that end up off
  was looked at: helpers that only dropped drivers use (HDMI and ELD
  helpers, ATA and SCSI parts, PCI sound), tests, and workarounds for
  errata in CoreSight's trace buffer, which the M1 does not have. The boot
  path (Btrfs, the EFI stub, simpledrm, the Apple drivers, the initramfs)
  is unchanged.
- A trial build of 7.2.14_p1 with it (`make all`, as the ebuild compiles)
  finished without an error or a modpost warning. Every driver in
  required.txt was built, as a module or into the kernel, and so was every
  module loaded on this Mac except `ac97_bus` and `snd_compress`, which
  `snd_soc_core` only needed for other boards' sound drivers.

Rebuilding the kernel version that is running replaces its modules with
ones built for the smaller config, and the running kernel would load them
without noticing the difference (the stock config checks neither module
signatures nor symbol versions): reboot right after that build, before
plugging in anything new.

### When a new kernel comes

A newer kernel's new drivers build until the list is refreshed, and the
names it no longer has are ignored. Two things can go wrong.

A new driver can call a helper that its own Kconfig entry does not select,
and that only drivers on the list do. The stock config hides the mistake;
with the list the build stops at its end, in modpost. That was 7.3.3_p2:

```
ERROR: modpost: drivers/gpu/drm/panel/panel-chipone-icna35xx.ko: symbol 'drm_dsc_pps_payload_pack' undefined!
ERROR: modpost: drivers/gpu/drm/panel/panel-novatek-nt36536.ko: symbol 'drm_dsc_pps_payload_pack' undefined!
```

Two panel drivers new in 7.3, for a handheld and a tablet, use the DSC
helper (`DRM_DISPLAY_DSC_HELPER`) without selecting it, and every driver
that does select it is on the list. Such a driver joins the list, as
these two have; then `sudo sh kernel/install.sh` and the build again.

The other is a driver this Mac needs coming to depend on something the
list turns off, which `olddefconfig` would drop without a word. So once
the config is final, the hook checks it against required.txt, before
anything is compiled, and stops the build if an option is off:

```
 * wmutils: the kernel config leaves out what this Mac needs:
 *   CONFIG_DRM_APPLE (appledrm)
```

The option's Kconfig entry says what it depends on (Portage leaves the
source under `/var/tmp/portage` after a failed build); delete the line in
trim.config that turns that off, and run `sudo sh kernel/install.sh`
again. To build anyway:

```sh
sudo env WMUTILS_UNCHECKED=1 emerge --oneshot sys-kernel/asahi-kernel
```

An option in required.txt that the new kernel does not have at all
(renamed, say) only gets a warning: check that its module still builds,
and update required.txt.

### Refreshing the list

The list was made for 7.2.14_p1 and refreshed for 7.3.3_p2. A refresh
starts from the config of the old kernel and of the new one, each put
together as the ebuild does it (Fedora's config, the Gentoo and Asahi
fragments, the list, `olddefconfig`); for 7.2.14 that comes out as the
running kernel's `/proc/config.gz`, option for option.

- Every option that is on in the new kernel with the list, and was not in
  the old one, is looked at. For 7.3.3 that was 83. 30 of them went on the
  list, by the rules above, and with the two panels that did not build
  the list has 32 new lines; they and what depends on them turn 46 of the
  83 off:
  - chips on I2C and SPI: sensors and converters, two chargers, an LED and
    a backlight controller, a haptic driver, three audio codecs;
  - five more display panels;
  - a Wi-Fi chip on SDIO and one on PCI, and a RAID card;
  - the calls into the firmware of Qualcomm SoCs, which 7.3 offers on
    every arm64 kernel and Fedora compiles in. In 7.2 only Qualcomm's own
    drivers turned them on.
- The other 37 stay: two HID drivers, two Ethernet PHYs and the library
  for MCTP over USB, which a USB device can need; generic parts of the
  kernel (crypto, memory management, IOMMU page tables, Rust bindings,
  options of Landlock, AppArmor, EROFS and the like); two workarounds for
  errata of other CPUs, like the ones already there; and a serial card on
  PCIe, as the list leaves serial ports alone.
- What the list turns off without naming it, because it depends on
  something named, is compared between the two kernels. 7.3.3 has 26 such
  options more: tests, PCI Ethernet, HD Audio and SoundWire parts,
  hardware monitors and camera sensors, whose menus are on the list, and
  the USB audio offload to Qualcomm's sound DSP. Of the options compiled
  into the stock kernel, 373 end up off: the 368 of 7.2.14 and five of
  the Qualcomm firmware calls.
- Nothing is on that is off in the stock config, and the 115 options in
  required.txt keep their stock values.
- This directory's four patches fit 7.3.3_p2 as they are.

### Turning something back on

Delete its line from trim.config and run `sudo sh kernel/install.sh`
again; the next build has it. The option gets its stock value back, and so
does everything under it: without `# CONFIG_ETHERNET is not set` all the
PCI Ethernet drivers return. The likely ones:

- `CONFIG_ETHERNET` and `CONFIG_ATA`, once the Asahi kernel tunnels PCIe
  over Thunderbolt, for docks and enclosures with a PCI Ethernet or SATA
  chip. NVMe enclosures need nothing: NVMe stays.
- `CONFIG_MEDIA_ANALOG_TV_SUPPORT`, `CONFIG_MEDIA_DIGITAL_TV_SUPPORT` and
  `CONFIG_MEDIA_RADIO_SUPPORT`, for a USB TV or radio stick.
- `CONFIG_TARGET_CORE`, for vhost-scsi disks in virtual machines or an
  iSCSI target (with targetcli).

Without `/etc/kernel/config.d/wmutils-trim.config` the build has
the full stock config again, until install.sh puts it back.

## Cores powered down sooner

The cpuidle driver has two states: `WFI`, where an idle core is
clock-gated but stays powered, and `CPU PD`, where it is powered down
and resumes where it stopped. `CPU PD` is left within 10 microseconds,
but the driver asked for 10 ms of idle time before entering it, so the
governor kept a core in `WFI` whenever its next timer, or the idle time
it predicted, was under 10 ms. A core that a 120 Hz display (8.3 ms) or
a short audio period wakes never powered down. On this machine, over
three hours that included a long compile, the first efficiency core
spent 5377 s in `WFI` and 3836 s in `CPU PD`, and 56 % of its `CPU PD`
entries ended early.

The patch asks for 2 ms. Entering the state costs the kernel's
`cpu_pm_enter()` and `cpu_pm_exit()` and saving a few registers, so
10 ms was far on the safe side; but where the state breaks even is not
documented and has not been measured, and a value that is too low costs
energy instead of saving it. So the patch also makes it a parameter,
which takes effect at once:

```sh
cat /sys/module/cpuidle_apple/parameters/pd_residency_us        # 2000
echo 10000 | sudo tee /sys/module/cpuidle_apple/parameters/pd_residency_us   # as before
```

`cpuidle_apple.pd_residency_us=10000` on the kernel command line does
the same from boot. To compare two values, leave the machine alone with
the screen at one brightness and average
`/sys/class/power_supply/macsmc-battery/power_now` (microwatts, negative
while discharging) over a few minutes with each. How much time each
state gets is in `/sys/devices/system/cpu/cpu*/cpuidle/state*/time`.

## Wi-Fi asleep sooner

In power save the Broadcom firmware keeps the radio awake for a while
after each frame (`pm2_sleep_ret`) before it sleeps until the next
beacon. brcmfmac sets that time from cfg80211's timeout, limited to
2000 ms; but cfg80211 passes -1 unless a timeout was asked for, which
only the old wireless extensions can do, and the driver's limit turned
-1 into the full 2000 ms. With a frame every second or two (broadcasts,
keepalives) the radio hardly slept while connected.

The patch uses 200 ms when no timeout was asked for. What that saves
depends on the traffic and has not been measured. The cost is latency: a
reply that comes more than 200 ms after the last frame waits for the
next beacon, typically up to 100 ms. It is a parameter too, read
whenever power save is set, which happens each time the interface comes
up:

```sh
cat /sys/module/brcmfmac/parameters/ps_timeout_ms               # 200
echo 2000 | sudo tee /sys/module/brcmfmac/parameters/ps_timeout_ms   # as before
nmcli radio wifi off && nmcli radio wifi on                     # takes the interface down and up
```

Power save itself is NetworkManager's `wifi.powersave` setting; with it
off (`2`) the radio never sleeps and the time does not matter.

## Parts that sleep while nothing is plugged in

The SD card reader, the HDMI port's converter chip and the three USB-C
ports' controllers stay powered whether or not anything is plugged into
them. Patches 0007, 0008 and 0009 let the kernel power each of them down
while it is unused. They change how hardware is driven, and whether the
hardware goes along (a wake-up signal that has to arrive from a part
that is asleep) can only be found out on the machine. So in the kernel
each one does nothing until it is switched on, and each switch works on
a running system and can be switched back.

`install.sh` switches all three on from boot, with
[unplugged.conf](unplugged.conf) (the reader and the converter) and
[unplugged.rules](unplugged.rules) (the ports). And two more, which
work on this machine but saved nothing that could be measured: patch
0010, which releases
[the HDMI port's PHY](#the-hdmi-ports-phy-and-crossbar) along with the
converter (unplugged.conf), and patch 0011, which has nothing to do
with plugs: it lets [the I2C buses](#the-i2c-buses) sleep between
transfers (unplugged.rules). After a boot:

```sh
cat /sys/module/sdhci_pci/parameters/gl9755_runtime_pm /sys/module/appledrm/parameters/hdmi_idle_off \
    /sys/module/appledrm/parameters/hdmi_idle_phy_off                                                  # Y, three times
cd /sys/bus/platform/devices
cat 702280000.usb/power/control 703000000.phy/power/control b02280000.usb/power/control \
    b03000000.phy/power/control f02280000.usb/power/control f03000000.phy/power/control                # auto, six times
cat 39b040000.i2c/power/control 39b044000.i2c/power/control 39b048000.i2c/power/control \
    39b04c000.i2c/power/control                                                                        # auto, four times
```

To leave one of them off, delete its option from unplugged.conf, or its
names from unplugged.rules (or the whole rule), and run
`sudo sh kernel/install.sh` again; a kernel build after that takes it
out of the initramfs too.

What has been seen on this machine (14-inch M1 Pro, `asahi-7.3.3-2`),
with all three on and only the charger plugged in: the reader in D3hot,
the converter powered down without coming back by itself, the three
ports' controllers and PHYs suspended, and their power domains
(`atc0_usb` to `atc2_usb`, `atc0_common` to `atc2_common`) off. What
has not been tried with them on: plugging in a card, a monitor or a USB
device, and a suspend and resume.

Together they save about 0.67 W, 18 % of what this machine draws idle
on battery with the panel at 2 nits and Wi-Fi connected: 3.64 W with
the three switched off, 2.97 W with them on (medians over 10 pairs of
50 s; every pair came out between 0.63 and 0.72 W). How much of that
each of the three accounts for has not been measured. The reading of
the whole machine's power moves by about 0.1 W by itself, and by more
when something runs, so one reading before and one after show little.
[unplugged-ab.sh](unplugged-ab.sh) flips the switches off and on in
turns, a minute each, reads the SMC's "Total System Power" once a
second, and gives the difference over the pairs with its standard
error:

```sh
sudo sh kernel/unplugged-ab.sh                # 6 pairs of 60 s, all three
sudo sh kernel/unplugged-ab.sh 10 60 usb      # 10 pairs, the USB-C ports only
sudo sh kernel/unplugged-ab.sh 6 60 hdmiphy   # the HDMI port's PHY and crossbar only (0010)
sudo sh kernel/unplugged-ab.sh 6 60 i2c       # the I2C buses only (0011)
```

Run it on battery with nothing plugged in, and leave the machine alone
meanwhile. It puts every switch back where it was, also when stopped
with Ctrl-C.

When trying those, have nothing unsaved, and keep `sudo dmesg -w` open
in a terminal. Each section below has the switch for a running system,
what to look for, and the way back.

### The SD card reader

The reader is a Genesys GL9755 on its own PCIe port
(`/sys/bus/pci/devices/0000:02:00.0`). sdhci-pci lets a reader sleep
only if its part of the driver says it can, and the GL9755's did not, so
writing `auto` to its `power/control` changed nothing. With the patch
and the parameter on, the reader goes to sleep (PCI D3hot) while the
slot is empty, with the controller's "wake on card insertion" set, so
that putting a card in wakes it through a PCIe power management event.
With a card in the slot it stays awake, as before, so
reading and writing a card are not touched.

```sh
sudo modprobe -r sdhci_pci && sudo modprobe sdhci_pci gl9755_runtime_pm=1
cat /sys/bus/pci/devices/0000:02:00.0/power/runtime_status      # suspended, with the slot empty
```

Put a card in: it has to show up in `lsblk` within a second or two, and
the status reads `active`. Take it out (unmounted): `suspended` again.
The open question is whether the wake-up arrives. The PCI core also
looks for it once a second until it has seen the first real one, so a
card that shows up only after a pause, or not at all, is the sign that
it does not. The PCIe port above the reader (`0000:00:01.0`) may go to
sleep too once the reader has; if cards are found only with
`echo on | sudo tee /sys/bus/pci/devices/0000:00:01.0/power/control`,
the wake-up does not get through a sleeping port.

Back to how it was: `echo on | sudo tee
/sys/bus/pci/devices/0000:02:00.0/power/control`, or load the module
again without the parameter. unplugged.conf sets the parameter at boot.

### The HDMI converter

The HDMI port is fed by a DisplayPort-to-HDMI converter chip, which the
driver powers up when it starts and never powers down. With
`hdmi_idle_off` on, the driver switches the converter off three seconds
after it finds the port empty and no picture going to it. When the
port's hot-plug line rises it switches the converter on, gives it
`hdmi_boot_ms` (500 ms) to start, and brings the link up as before. The
5 V supply of the port stays on. The port's PHY is the next section's.

```sh
echo 1 | sudo tee /sys/module/appledrm/parameters/hdmi_idle_off
```

With the port empty, `dmesg` says `DP2HDMI powered down` right away.
Plug a monitor in: `DP2HDMI powered up`, then the monitor comes up as it
always did, and `cat /sys/class/drm/card*-HDMI-A-1/status` reads
`connected`. Unplug it: `powered down` again, once, about three seconds
later. Try it a few times, and across a suspend.

The open question is whether the hot-plug line still reaches the SoC
while the converter has no power. If it does not, a monitor is simply
not noticed; if the line floats, the two messages repeat every few
seconds. Either way:

```sh
echo 0 | sudo tee /sys/module/appledrm/parameters/hdmi_idle_off
```

powers the converter up at once and leaves it up, as without the patch.
If a monitor is noticed but stays black the first time, the converter
may need longer to start: `echo 1000 | sudo tee
/sys/module/appledrm/parameters/hdmi_boot_ms` (up to 5000).
unplugged.conf sets `hdmi_idle_off` at boot; appledrm loads in the
initramfs, so a change to it counts from the next kernel build on.

### The HDMI port's PHY and crossbar

Behind the converter the port is a DisplayPort output of the SoC. Three
parts make it: a display coprocessor of its own (`289c00000.dcp`), the
display crossbar (`130304c000.mux`), which connects a coprocessor to a
PHY, and the fourth Type-C PHY (`1303000000.phy`), which drives the
lines to the converter. The driver connects the crossbar and switches
the PHY to DisplayPort when it starts and leaves them so, monitor or
not: the PHY is always in use, and the power domain the two share
(`atc3_usb`) never goes off.

With `hdmi_idle_phy_off` on as well as `hdmi_idle_off`, the driver
releases them where it powers the converter down: it switches the PHY
off and disconnects the crossbar. Both can then sleep, and their power
domain goes off. When the hot-plug line rises it connects the crossbar
and switches the PHY to DisplayPort again, before the converter gets
its power. The crossbar had no runtime power management; with the patch
it has, and it sets itself up again after sleeping the way its probe
does.

The coprocessor keeps running. Its power domain is marked always-on in
the device tree, and the driver has no way to shut a coprocessor down
and start it again (its own comment at suspend: "we do not support full
shutdown properly yet"). With no monitor it is told to power its
display pipeline off, which it was before the patch too.

`install.sh` switches this one on too, with unplugged.conf. On this
machine it does what it says and saves nothing that could be measured:
with the parameter on
both devices were `suspended` in every half of a run of
`unplugged-ab.sh 6 60 hdmiphy`, and by the medians of five pairs the
machine drew 0.03 W more with it on than with it off, give or take 0.02
(at 4.2 W, with the panel at 125 nits; the first pair, three minutes
after a boot with the CPU still busy, is left out). The domain above
theirs (`atc3_common`) is always-on in the device tree and stays on.
No monitor has been plugged in with it on. unplugged.rules allows the two devices to
sleep, which changes nothing while they are in use. By hand, with nothing in
the HDMI port, nothing unsaved and `sudo dmesg -w` open:

```sh
cat /sys/bus/platform/devices/1303000000.phy/power/control /sys/bus/platform/devices/130304c000.mux/power/control   # auto, auto
sync; echo 1 | sudo tee /sys/module/appledrm/parameters/hdmi_idle_phy_off
cat /sys/bus/platform/devices/1303000000.phy/power/runtime_status /sys/bus/platform/devices/130304c000.mux/power/runtime_status   # suspended, suspended, a second or two later
```

`dmesg` says `Switched dpphy to disconnected state` and `DP path
released`. Plug a monitor in: `Switched dpphy to dispext0,0`, `DP path
connected`, `DP2HDMI powered up`, and the monitor comes up as it always
did. Unplug it: both messages of the release again, three seconds
later, and both statuses back to `suspended`.

What can go wrong: the coprocessor has not been seen to run with its
output disconnected. If it minds, `dmesg` says `DCP has crashed`, and
the HDMI port is dead until a reboot (the built-in panel has its own
coprocessor). A monitor that stays black after `DP path connected` says
that the link does not come up again from a PHY that was off. Either
way:

```sh
echo 0 | sudo tee /sys/module/appledrm/parameters/hdmi_idle_phy_off
```

connects both again at once and leaves them so, as without the patch.
To leave it off from boot, take `hdmi_idle_phy_off=1` out of the
`appledrm` line of unplugged.conf and run `sudo sh kernel/install.sh`
again; a kernel build after that takes it out of the initramfs too.

### The USB-C ports

Each USB-C port has a dwc3 USB controller and a Type-C PHY (atcphy).
Neither driver had runtime power management, and the kernel keeps the
power domain of such a device on for good. With the patch both have it:
the controller counts as in use from the moment a cable is plugged in
until it is pulled out, the PHY whenever it is in any other state than
the one it starts in, and when both are idle their power domains can be
switched off. Waking the PHY sets it up again the way its probe does. A
port with something in it is driven exactly as before.

This is the patch that can go wrong the hardest: a register access to a
part without power stops the machine on the spot. So the drivers start
with it forbidden (`power/control` reads `on`) until it is allowed, one
port at a time, which is what unplugged.rules does at boot:

```sh
cd /sys/bus/platform/devices
echo auto | sudo tee 702280000.usb/power/control 703000000.phy/power/control
cat 702280000.usb/power/runtime_status 703000000.phy/power/runtime_status   # suspended, about a second later
```

Then, on that port: a USB 2 device (keyboard, mouse), a USB 3 device (a
disk; `lsusb -t` shows 5000M or more), the charger, a hub, a display or
dock if you use one, each plugged in and pulled out a few times and in
both orientations, and a suspend and resume with and without a device
in it. After each unplug both statuses have to return to `suspended`.
`echo on` into the same two files puts the port back as it was, also
while it is asleep.

| Port | Controller | PHY |
|---|---|---|
| First | `702280000.usb` | `703000000.phy` |
| Second | `b02280000.usb` | `b03000000.phy` |
| Third | `f02280000.usb` | `f03000000.phy` |

Which socket is which shows when a device is plugged in: that port's
controller turns `active`. `1303000000.phy` is the fourth PHY, which
feeds the HDMI converter; it is in use, and awake, for as long as
[`hdmi_idle_phy_off`](#the-hdmi-ports-phy-and-crossbar) has not
released it. Whether the domains really go off is in
`sudo grep atc /sys/kernel/debug/pm_genpd/pm_genpd_summary`.

### The I2C buses

Four I2C buses are in use on this machine, each with a controller of
its own in the SoC and a power domain of its own (`i2c0` to `i2c3`):

| Bus | Controller | What is on it |
|---|---|---|
| First | `39b040000.i2c` | The four USB-C port controllers (cd321x) |
| Second | `39b044000.i2c` | Three speaker amplifiers (tas2764) |
| Third | `39b048000.i2c` | The headphone codec (cs42l84) |
| Fourth | `39b04c000.i2c` | The other three speaker amplifiers |

Those chips are written to when a cable goes in or a stream starts, and
otherwise not: while the machine sits idle the interrupt counts of the
four buses do not move. The controller's driver had no runtime power management, so
all four domains were on for good. With the patch a controller is awake
for a transfer and 100 ms after it, and its domain can go off in
between. Waking it sets it up again the way its probe does. Around a
system sleep it is kept awake from its own suspend callback to its
resume callback, so that the chips' drivers can reach them in all of
theirs.

This is not about plugs, and it has no part in the 0.67 W above. On
this machine it works and saves nothing that could be measured: with
`auto` all four buses were `suspended` through every half of a run of
`unplugged-ab.sh 6 60 i2c`, with not one transfer in twelve minutes,
and by the medians of the four middle pairs the machine drew 0.05 W
more with them asleep than awake, give or take 0.02 (at 6.1 W, with
about 4 % of the CPU in use throughout; the first and the last pair,
with bursts to 8 W and more in one half, are left out). The four
domains are small ones, at the end of the tree, and the domain above
them (`sio`) stays on for the keyboard and the trackpad. Like the USB-C
ports it starts forbidden (`power/control` reads `on`), until
unplugged.rules allows it at boot. Sound, the headphone jack, the ports
and a suspend have not been tried with it on. By hand, with nothing
unsaved and `sudo dmesg -w` open:

```sh
cd /sys/bus/platform/devices
sync; echo auto | sudo tee 39b040000.i2c/power/control 39b044000.i2c/power/control 39b048000.i2c/power/control 39b04c000.i2c/power/control
cat 39b040000.i2c/power/runtime_status 39b044000.i2c/power/runtime_status 39b048000.i2c/power/runtime_status 39b04c000.i2c/power/runtime_status   # suspended, four times
```

Then: sound from the speakers, started and stopped a few times;
headphones plugged in and pulled out, and sound through them; the
charger and a USB device plugged in and pulled out; a suspend and
resume, and all of that once more after it. A bus turns `active` for a
moment each time and has to return to `suspended`. What can go wrong: a
controller that does not answer after waking. `dmesg` then has I2C
errors from `i2c-apple`, `tas2764`, `cs42l84` or `tps6598x`, and there
is no sound, or a port does not notice its cable. `echo on` into the
same four files keeps the controllers awake again, as without the
patch. To leave it off from boot, delete the third rule of
unplugged.rules and run `sudo sh kernel/install.sh` again.

### What stays powered

Looked at for the same treatment, and left as they are:

- Audio. The driver of the I2S ports (`apple-mca`) already powers each
  port's domain for the length of a stream. What stays on at idle is
  the domain above them (`audio_p`), which the SIO coprocessor needs as
  well; the audio DMA controller (`apple-admac`, domain `sio_adma`); and
  the I2S port of the speakers' sense lines. That last one is held by a
  stream, not by a missing callback: speakersafetyd, which keeps the
  speakers from overheating, has the "Speaker Sense" capture stream open
  and running for as long as it lives, and the DMA controller has a
  channel started for it. Runtime PM in the DMA controller would
  therefore never find it idle. Letting that stream lose its clock and
  power while nothing plays means changing how the sound card starts
  and stops the sense path against the playback path, and a mistake
  there puts the sense data, which the speaker protection works from,
  out of step. Not done without first knowing what it would save.
- GPIO. Of the four pin controllers only `39b028000.pinctrl` has a power
  domain that can be switched (`gpio`), and it delivers the interrupts
  of the keyboard and trackpad, the HDMI hot-plug line, the headphone
  codec, the USB-C port controllers and the speaker amplifiers. Something
  is always waiting on it. `2921f0000.pinctrl` is in `nub_gpio`, which
  the device tree marks always-on; the other two have no power domain.
- The SMC (`290400000.smc`) and its mailbox have no power domain: there
  is nothing to switch.
- The SSD. Its controller is a coprocessor of the SoC (ANS2), and the
  driver (`nvme-apple`) offers none of NVMe's own power states (no
  APST). The one way it has of powering the SSD down is that of a
  system sleep: an NVMe shutdown, then a shutdown of the coprocessor,
  and on the way back a reset and a fresh boot of the coprocessor. The
  driver says of that boot that there is no known way to recover if it
  fails. With the root filesystem on it and something written every few
  seconds, doing that whenever the SSD has been idle for a while would
  rarely last long enough to save anything, and the failure it risks is
  the root filesystem gone until a reboot.
- The GPU (`406400000.gpu`, domain `gfx`, with `afr` above it). The
  driver (`asahi`) has no runtime PM, and the part that draws power does
  not wait for one: the GPU's firmware switches the GPU off by itself
  once it has been idle for the delay the driver hands it, and the driver
  hands it 2 ms (`idle_off_delay_ms` in `gpu.rs`). The driver's share is
  one message that wakes the firmware when there is work. Switching
  `gfx` off from the kernel would mean shutting that firmware down and
  booting it again, and building anew everything the driver shares with
  it, under the programs that are drawing. The driver has no shutdown
  path (its RTKit binding can boot and wake a coprocessor, not stop
  one).
- The SIO coprocessor (`39bc00000.sio`, domain `sio_cpu`, which in turn
  holds `spi_p` and `uart_p`). Its one user here is sound over HDMI. The
  driver (`apple-sio`) suspends the device whenever no such sound plays,
  but its suspend callback is empty (a TODO in the source), so the
  firmware keeps running and its mailbox (`39bc08000.mbox`) stays awake.
  Shutting the firmware down there would let the mailbox sleep and still
  not the domain: the two IOMMUs in it (`39b004000.iommu`,
  `39b008000.iommu`) also serve the audio DMA controller, which never
  rests (see Audio above). What is left to save is an idle coprocessor
  inside a domain that stays powered, against sound over HDMI gone until
  a reboot if this firmware does not boot a second time, which the
  driver has never asked of it. Not done.

## Fewer wakeups at idle

A CPU core that nothing wakes is powered down (the `CPU PD` state in
`/sys/devices/system/cpu/cpu0/cpuidle`), so an idle machine saves power by
being woken less often. [power.config](power.config) is a second config
fragment, merged like trim.config, that turns on three options the stock
config leaves off:

| Option | What changes |
|---|---|
| `CONFIG_WQ_POWER_EFFICIENT_DEFAULT` | Work items on the kernel's "power efficient" workqueues run on a CPU that is awake instead of waking the one that queued them. The same as booting with `workqueue.power_efficient=1` |
| `CONFIG_RCU_NOCB_CPU_DEFAULT_ALL` | RCU callbacks run in kernel threads (`rcuop/N`), which the scheduler places on a CPU that is awake, instead of in each CPU's softirq. The same as booting with `rcu_nocbs=all`; lazy RCU works only this way |
| `CONFIG_RCU_LAZY` | RCU callbacks that are in no hurry wait up to ten seconds to share a grace period, instead of each starting one at once. Memory pressure or a long callback list ends the wait early |

They are generic kernel code, written for battery-powered devices (lazy
RCU came from ChromeOS and Android), and touch no Apple driver. Merged
into the config of 7.3.3_p2 the way the ebuild merges it, the fragment
changes these three options and nothing else, and every option in
required.txt keeps its value. What they save on this machine has not been
measured. Under load they cost a little: an unbound work item or an
offloaded callback can run on another CPU than the one whose cache has its
data.

After booting a kernel with it:

```sh
cat /sys/module/workqueue/parameters/power_efficient   # Y
zcat /proc/config.gz | grep -E 'RCU_LAZY|RCU_NOCB_CPU_DEFAULT_ALL'
pgrep -c rcuop                                         # 8, one per CPU
```

To boot once without the first or the third, add
`workqueue.power_efficient=0` or `rcutree.enable_rcu_lazy=0` to the kernel
command line. To drop an option for good, delete its line from
power.config and run `sudo sh kernel/install.sh` again; the next build has
the stock value.

## The trackpad without interrupts

Where the interrupts of the devices go was counted over a minute each
(`/proc/interrupts`), with the performance cores in mind: an idle one is
powered down, and an interrupt wakes it.

| The machine | Device interrupts a second | Of them on the two efficiency cores |
|---|---|---|
| Left alone | 34 | 94 % |
| A program redrawing, nothing touched | 327 | 96 % |
| A finger moving on the trackpad | 1237 | 49 % |

With the finger on the trackpad, 1007 of the 1237 were the SPI
controller's that the keyboard and trackpad hang on (`39b10c000.spi`),
and 54 % of those went to one performance core, which went idle and was
woken again about 1600 times a second.

They cannot be told where to go. The interrupt controller of this SoC
(AIC2) picks the core itself: its driver has no affinity setting, so
`/proc/irq/N/smp_affinity` cannot be written, and the kernel does not
let the thread of an interrupt handler be moved (it is bound, and
`taskset` is refused). What can be changed is how many there are.

The trackpad sent 63 packets a second, so each took 16 interrupts. A
packet is 256 bytes; the controller's FIFO holds 16; and the driver
(`spi-apple`) sleeps until an interrupt for every wait it reckons longer
than 5 us. At the 8 MHz of this bus half a FIFO takes 8 us, so it
sleeps, sixteen times a packet, for a few microseconds each. The flash
chip on the other SPI bus runs at 25 MHz and is polled already.

[0012](0012-spi-apple-poll-threshold.patch) makes the 5 us a parameter.
At 8 the keyboard and trackpad are polled: the thread that reads a
packet stays on its core for the 256 us the packet is on the wire, and
the SPI controller raises no interrupt. That is a trade, not a plain
gain. The thread used 192 us of CPU time a packet with the interrupts
and will use the 256 us and more when polling; what goes away is the
interrupts and a core's going idle and being woken sixteen times a
packet. Which draws less has not been measured, and the patch has not
yet been booted.

With a kernel that has it:

```sh
cat /sys/module/spi_apple/parameters/poll_us              # 5
echo 8 | sudo tee /sys/module/spi_apple/parameters/poll_us
grep 39b10c000.spi /proc/interrupts    # no longer counts while you type or point
sudo sh kernel/trackpad-ab.sh          # 4 pairs of 20 s: keep a finger circling
```

`trackpad-ab.sh` switches between 5 and 8 in turns and prints for each
half the packets, the SPI interrupts and the share of them on the
performance cores, the reading thread's sleeps, how often the
performance and the efficiency cores went idle, and the power draw
(`Total System Power`). The finger is the load, so its evenness is the
measurement's: the packets column says how even it was. Nothing sets
the parameter at boot; it is back at 5 after a reboot.

## Applying them automatically on Gentoo

```sh
sudo sh kernel/install.sh
sudo emerge --oneshot sys-kernel/asahi-kernel   # now, or wait for the next update
```

and reboot. `install.sh` puts the patches and required.txt in
`/usr/local/share/wmutils` (removing any patch there that left the
series), a Portage hook in `/etc/portage/env/sys-kernel/asahi-kernel`,
trim.config and power.config in `/etc/kernel/config.d` (as
`wmutils-trim.config` and `wmutils-power.config`),
appledrm.conf and unplugged.conf in `/etc/modprobe.d` (as
`wmutils-appledrm.conf` and `wmutils-unplugged.conf`), and
unplugged.rules in `/etc/udev/rules.d/60-wmutils-unplugged.rules`. The
kernel build's dracut run copies those three into the initramfs, where
appledrm and the USB-C drivers are loaded, and its m1n1 hook puts the
new DTB in the boot loader.
From then on every `asahi-kernel` build, kernel updates included, runs the
hook after the kernel's own patches. For each patch in turn it applies the
patch if it fits, skips it if the source already has it, and otherwise
warns and builds without it. A kernel update therefore never fails because
of a patch; a plain `/etc/portage/patches` entry would stop the build
instead, so `install.sh` removes one if it finds it. The ebuild merges
trim.config and power.config into the config, and once it is final the hook checks it
against required.txt (see [When a new kernel comes](#when-a-new-kernel-comes)).
`install.sh` will not overwrite a hook file it did not write. Run it again
after anything in this directory changes. `sudo sh kernel/install.sh
remove` undoes it all.

If a build warns that a patch no longer fits, that patch is missing until it
is updated: without 0001 the backlight counts whole nits (and
[autobrightd](../autobrightd) shows 1-nit steps), without 0002 brightness
changes are back to about 21 updates a second, without 0003 the PMP stays
off, without 0004 the display runs at a fixed rate (appledrm then
ignores `force_vrr` with a warning in `dmesg`), without 0005 a core
powers down only for idle periods of 10 ms and more, and without 0006
the Wi-Fi radio stays awake 2 s after traffic (the two parameters are
then missing from `/sys/module`). Without 0007, 0008, 0009, 0010 or 0011 that
part stays powered as it does today, and its switch is missing or, for
the USB-C ports and the I2C buses, `runtime_status` reads `unsupported`.
Without 0012 `/sys/module/spi_apple/parameters/poll_us` is missing. A kernel that already has
a patch's change is detected and the patch skipped, so once the Asahi
kernel ships VRR or turns the PMP on, nothing breaks.

The patches are changes to the Linux kernel, not part of this
repository's GPL-3.0-or-later code, and each is under the license of
the files it changes (the `SPDX-License-Identifier` line at the top of
each):

| Patch | Files | License |
|---|---|---|
| 0001, 0002, 0004, 0008, 0010 | `drivers/gpu/drm/apple/` | GPL-2.0-only OR MIT |
| 0003 | the devicetree | GPL-2.0+ OR MIT |
| 0005 | `drivers/cpuidle/cpuidle-apple.c` | GPL-2.0-only OR MIT |
| 0006 | brcmfmac's `cfg80211.c` | ISC |
| 0007 | `drivers/mmc/host/sdhci-pci-gli.c` | GPL-2.0+ |
| 0009 | `drivers/phy/apple/atc.c` | GPL-2.0 OR BSD-2-Clause |
| 0009 | `drivers/usb/dwc3/dwc3-apple.c` | GPL-2.0 |
| 0010 | `drivers/mux/apple-display-crossbar.c` | GPL-2.0-only OR MIT |
| 0011 | `drivers/i2c/busses/i2c-pasemi-*` | GPL-2.0-only |
| 0012 | `drivers/spi/spi-apple.c` | GPL-2.0 |

They were written from the kernel's own source and the public SD Host
Controller specification only; nothing in them comes from macOS or any
other Apple code.
