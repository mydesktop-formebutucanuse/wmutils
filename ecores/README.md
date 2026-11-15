# ecores

Keeps background work on the efficiency cores, so that it does not wake
the performance cores or run them at speed. Two kinds of work, each in a
cgroup of its own whose `cpuset.cpus` says where it may run:

| cgroup | What is in it | Where it runs |
|---|---|---|
| `wmutils-daemons` | The system's daemons and what they start | On the efficiency cores, always |
| `wmutils-batch` | Work that may take long: emerge and its builds, and what you put there | On the efficiency cores while on battery, on every core while the charger is plugged in |

The login session, with X and every program started from it, is in
neither and runs where the scheduler puts it, as before.

| File | What it is |
|---|---|
| [install.sh](install.sh) | Installs the rest and moves the daemons at once; `portage` makes emerge batch work |
| [ecores.sh](ecores.sh) | Makes the cgroups, moves processes into them, sets the cores, and shows what is where |
| [ecores.start](ecores.start) | Runs `ecores.sh start` at every boot, from `/etc/local.d` |
| [ecores.rules](ecores.rules) | udev rule: runs `ecores.sh power` when the charger is plugged in or out |

Written for a MacBook Pro M1 Pro (2 efficiency and 6 performance cores)
on Gentoo with s6-rc and elogind, kernel 7.3 with cgroup v2. The
efficiency cores are found, not named: they are the CPUs whose
`cpu_capacity` is below the largest. On a machine whose CPUs are all alike
`install.sh` stops and installs nothing.

```sh
sudo sh ecores/install.sh           # the daemons, now and at every boot
sudo sh ecores/install.sh portage   # emerge too, while on battery
sudo sh ecores/install.sh remove    # undo both, at once
```

## How it was

The kernel already schedules by energy (`sched_energy_aware` is 1): a task
that fits an efficiency core is put on one. Over 30 seconds of light use on
battery, about 2660 of the 2850 wakeups a second were on the two efficiency
cores, and the six performance cores spent 95 to 99 % of the time powered
down. The daemons were hardly among the tasks that woke anything.

So this is not what makes an idle machine draw less: expect little there,
and see [Measuring](#measuring). What the scheduler does not have is a
notion of work that is in no hurry. A build, an upload or a scan that
keeps a core busy does not fit an efficiency core, so the scheduler moves
it to a performance core and runs that at its top frequency, whether or
not anyone waits for the result. The two cgroups are that notion.

## The daemons

Every daemon is a child of the s6 supervision tree, which is in the root
cgroup; a login session is in a cgroup of its own, `/1` for the first,
where elogind puts it. `ecores.sh start` moves every process of the root
cgroup into `wmutils-daemons`. A process started later by one of them (a
restarted daemon, a udev worker, a program D-Bus starts) is born there. A
login leaves it: elogind moves the session into its own cgroup, and the
cores are again all of them.

Two things stay in the root cgroup:

- Process 1, `s6-svscan`. It only waits for its children, and elogind
  takes the cgroup of process 1 for the place under which it makes the
  sessions' cgroups: with process 1 moved, a login would land inside
  `wmutils-daemons`, on two cores.
- Kernel threads. Where they run is the kernel's choice, and many are tied
  to one CPU.

With this, what a daemon does at length stays off the performance cores:
rclone encrypting an upload, NetworkManager and wpa_supplicant during a
scan, udev when something is plugged in. It is slower for it, on cores
with under half the capacity. Files read through an rclone mount are the
one place where that may be felt.

## Batch work

`wmutils-batch` is on the efficiency cores only while the machine runs
from its battery. The kernel sends a udev event when the charger is
plugged in or out, [ecores.rules](ecores.rules) runs `ecores.sh power`,
and a build that is under way moves over to all the cores, or back to
two, within the second.

On battery this is a trade. By the kernel's own figures the two efficiency
cores have a seventh of the machine's capacity (485 each, the six others
1024 each), so a build that fills every core takes about seven times as
long. What it saves is the performance cores at full speed, by far the
largest draw this machine has. That is worth it when the machine is on
anyway and nobody waits for the build, and not when the build is the
reason it is on: the screen and the rest then draw for seven times as
long. How much either way has not been measured here. That is why
`install.sh` leaves emerge alone until asked:

```sh
sudo sh ecores/install.sh portage
```

It adds two lines to `/etc/portage/make.conf`:

```sh
# wmutils: added by ecores/install.sh portage; emerge and its builds enter the wmutils-batch cgroup.
PORTAGE_IONICE_COMMAND="/usr/local/share/wmutils/ecores/ecores.sh enter \${PID}"
```

emerge runs that command with its own process ID when it starts, and
everything it starts is born in the cgroup. Portage has one such command:
if `make.conf` sets another already, `install.sh` says so and changes
nothing. For one build on every core while on battery, plug the charger
in, or:

```sh
sudo env PORTAGE_IONICE_COMMAND= emerge --oneshot PACKAGE
```

Anything else becomes batch work the same way, by its process ID. For a
shell and all that is started from it afterwards:

```sh
sudo /usr/local/share/wmutils/ecores/ecores.sh enter $$
```

Without root, `taskset -c 0,1 COMMAND` keeps a command on the efficiency
cores too (0 and 1 on this machine), but always: nothing gives it the other cores when the charger
is plugged in.

A whole `-j8` build on two cores does not take them from everything else.
The scheduler shares a core between cgroups before it shares within one:
the build as a whole counts as much as the daemons together, or as the
whole login session. And once the efficiency cores are full the scheduler stops
placing tasks by energy, so the session's tasks go to the idle
performance cores.

## What it leaves alone

- The login session: X, the window manager, PipeWire and the speaker DSP,
  the input method, every application. Their cores are a matter of how
  the desktop feels and sounds, and the scheduler already keeps their
  small tasks on the efficiency cores.
- Kernel threads and interrupts. The Wi-Fi interrupt and some work queues
  do wake performance cores, a few times a second each. Moving them is
  possible (`/proc/irq/N/smp_affinity`,
  `/sys/devices/virtual/workqueue/cpumask`) but it changes how fast the
  network and the screen answer, and would need measuring first.

## Measuring

`ecores.sh status` shows the two cgroups, their cores and what is in
them. `ecores.sh idle` counts, for every core, how often it left idle and
how much of the time it spent in its deepest idle state, powered down;
neither needs root:

```sh
/usr/local/share/wmutils/ecores/ecores.sh status
/usr/local/share/wmutils/ecores/ecores.sh idle 60
```

To see what the daemons' cgroup changes, compare the performance cores'
lines with it and without, the machine left alone both times:

```sh
sudo /usr/local/share/wmutils/ecores/ecores.sh stop
/usr/local/share/wmutils/ecores/ecores.sh idle 60
sudo /usr/local/share/wmutils/ecores/ecores.sh start
/usr/local/share/wmutils/ecores/ecores.sh idle 60
```

For batch work the difference is in watts and in minutes: the same build
once on battery with the charger out, once with
`PORTAGE_IONICE_COMMAND=` as above, and the battery's charge before and
after each.

## Remove

`sudo sh ecores/install.sh remove` takes the two lines out of
`make.conf`, moves every process of the two cgroups back to the root
cgroup, removes the cgroups and deletes what was installed. A process
that had entered `wmutils-batch` from a login session ends up in the root
cgroup, not back in the session's; it runs on every core either way.
