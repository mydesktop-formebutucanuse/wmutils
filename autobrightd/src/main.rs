// SPDX-License-Identifier: GPL-3.0-or-later
//! autobrightd: ambient-light auto-brightness for Apple Silicon Macs on
//! Asahi Linux. Drives the screen and keyboard backlights from the AOP
//! ambient light sensor and steps aside when someone else changes them.
//!
//! It wakes 5 times a second while nothing changes (the sensor reports every
//! 200 ms), and about 60 times a second while a change runs. Manual
//! screen changes are noticed through sysfs_notify() on actual_brightness,
//! and plugging or unplugging the charger through kernel uevents.
//!
//! What the backlights were set to is saved (state.rs) and put back when
//! the daemon starts, so it survives a restart of the machine, and put back
//! after a sleep, whatever the closing lid did to the light.

/// eprintln! that ignores errors. With panic = "abort", a closed or broken
/// stderr (say, the terminal that started the session is gone) would
/// otherwise kill the daemon at its next message.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod calibrate;
mod logic;
mod state;
mod sys;

use logic::{Channel, Kept, LuxFilter, Settings, Update};
use std::path::Path;
use std::process::exit;
use std::rc::Rc;
use std::time::{Duration, Instant};
use sys::{Attr, KeyEvent, KeySignals, Uevents};

const ALS_NAME: &str = "aop-sensors-als";
const SCREEN_DIR: &str = "/sys/class/backlight/apple-panel-bl";
const KBD_DIR: &str = "/sys/class/leds/kbd_backlight";
/// Whether the charger is connected. macsmc-ac/online cannot tell: on an
/// M1 Pro it drops to 0 for about 0.4 s when the cable goes in or out and
/// reads 1 otherwise. The battery status is right, but each read costs about
/// 0.8 ms of SMC traffic, so it is read only when the SMC driver sends a
/// power_supply uevent (on every plug or unplug).
const BATTERY_STATUS: &str = "/sys/class/power_supply/macsmc-battery/status";
const POWER_SUPPLY_PREFIX: &str = "macsmc-";
/// The status is read again this long after an event, in case it lagged.
const POWER_RECHECK: Duration = Duration::from_millis(1500);
/// Without uevents, the status is polled this often instead.
const POWER_POLL: Duration = Duration::from_secs(5);

/// The sensor reports every 200 ms; reading it more often finds the same value.
const SAMPLE: Duration = Duration::from_millis(200);
/// While the panel is off, only DPMS is checked, this often.
const DPMS_OFF_CHECK: Duration = Duration::from_secs(1);
/// The time asleep (sys::time_asleep) growing by this much is a sleep. It
/// only ever grows by whole sleeps, which take seconds.
const SLEEP_MIN: Duration = Duration::from_millis(500);

const USAGE: &str = "usage: autobrightd [-v] [-n] | --calibrate
  -v           print what it does
  -n           dry run: read the sensor and print, but write nothing
  --calibrate  measure how the panel follows the brightness written (about
               70 s; autobrightd must not run) and save it for later runs";

struct Backlight {
    channel: Channel,
    attr: Attr,
}

struct Daemon {
    als: Attr,
    screen: Backlight,
    /// actual_brightness of the screen, only for its change notification.
    screen_notify: Option<Attr>,
    kbd: Option<Backlight>,
    battery: Option<Attr>,
    uevents: Option<Uevents>,
    /// Brightness keys, as SIGUSR1 (brighter) and SIGUSR2 (dimmer).
    keys: Option<KeySignals>,
    /// When to read the battery status next, if at all.
    power_check: Option<Instant>,
    dpms: Option<Attr>,
    verbose: bool,
    dry_run: bool,
    filter: LuxFilter,
    lux: f32,
    on_ac: bool,
    panel_off: bool,
    /// sys::time_asleep() when last looked at; more means a sleep since.
    asleep: Duration,
    /// After a sleep, the light changes nothing until then.
    hold_until: Option<Instant>,
    /// What the state file holds (screen, keyboard), if written.
    saved: Option<(Kept, Option<Kept>)>,
}

/// Sets a backlight to `to` at once, without a change in steps: after a
/// restart or a sleep there is nothing on the screen to fade from.
fn set_now(b: &mut Backlight, to: u32, dry_run: bool) {
    if to == b.channel.known {
        return;
    }
    if !dry_run && let Err(e) = b.attr.write_u32(to) {
        say!("autobrightd: writing {}: {e}", b.attr.path.display());
        b.channel.stop();
        return;
    }
    b.channel.wrote(to);
}

fn mode(kept: Kept) -> &'static str {
    if kept.override_lux.is_some() { "set by hand" } else { "automatic" }
}

fn open_backlight(name: &'static str, dir: &str, settings: Settings, write: bool) -> Result<Backlight, String> {
    let attr = Attr::open(Path::new(dir).join("brightness"), write).map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::PermissionDenied {
            "\n  install 90-autobrightd.rules (see README) and be in the video group, or use -n"
        } else {
            ""
        };
        format!("{dir}/brightness: {e}{hint}")
    })?;
    let current = attr.read_u32().map_err(|e| format!("{}: {e}", attr.path.display()))?;
    Ok(Backlight { channel: Channel::new(name, settings, current), attr })
}

fn max_brightness(dir: &str) -> Result<u32, String> {
    let path = Path::new(dir).join("max_brightness");
    sys::read_once(&path)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&m| m > 0)
        .ok_or_else(|| format!("cannot read {}", path.display()))
}

impl Daemon {
    fn open(verbose: bool, dry_run: bool, keys: Option<KeySignals>) -> Result<Daemon, String> {
        let write = !dry_run;
        let als_dir = sys::find_iio(ALS_NAME).ok_or_else(|| {
            format!(
                "no IIO device named {ALS_NAME}\n  is /lib/firmware/apple/aop-als-cal.bin installed? \
                 If it was added after boot, reboot once"
            )
        })?;
        let als = Attr::open(als_dir.join("in_illuminance_input"), false)
            .map_err(|e| format!("{}: {e}", als_dir.display()))?;

        let screen_max = max_brightness(SCREEN_DIR)?;
        let screen_settings = Settings {
            curve: logic::SCREEN_CURVE,
            floor_pct: logic::SCREEN_FLOOR_PCT,
            motion: logic::SCREEN_MOTION,
            max: screen_max,
            dead_zone: (screen_max as f32 * logic::SCREEN_DEAD_PCT / 100.0) as u32,
        };
        let mut screen = open_backlight("screen", SCREEN_DIR, screen_settings, write)?;
        match calibrate::load(screen_max) {
            Ok(map) => screen.channel.map = map.map(Rc::new),
            Err(e) => say!("autobrightd: panel map: {e}"),
        }
        let screen_notify = Attr::open(Path::new(SCREEN_DIR).join("actual_brightness"), false).ok();
        // Read once so the notification is armed.
        if let Some(n) = &screen_notify {
            let _ = n.read_int();
        }

        let mut kbd = match max_brightness(KBD_DIR) {
            Ok(max) => {
                let settings = Settings {
                    curve: logic::KBD_CURVE,
                    floor_pct: logic::KBD_FLOOR_PCT,
                    max,
                    motion: logic::KBD_MOTION,
                    dead_zone: 0,
                };
                Some(open_backlight("kbd", KBD_DIR, settings, write)?)
            }
            Err(_) => None,
        };
        // Back to what they were set to when the daemon last ran. A value
        // for another max_brightness (other kernel patches) is left alone.
        let saved = state::load().unwrap_or_else(|e| {
            say!("autobrightd: {e}");
            Vec::new()
        });
        for b in std::iter::once(&mut screen).chain(kbd.as_mut()) {
            let max = b.channel.settings.max;
            let entry = saved.iter().find(|e| e.name == b.channel.name && e.max == max && e.kept.target <= max);
            if let Some(entry) = entry {
                let from = b.channel.known;
                let to = b.channel.restore(entry.kept);
                set_now(b, to, dry_run);
                if verbose {
                    say!("autobrightd: {} {from} -> {to} as saved ({})", b.channel.name, mode(entry.kept));
                }
            }
        }

        let battery = Attr::open(BATTERY_STATUS, false).ok();
        let uevents = battery.as_ref().and_then(|_| Uevents::open().ok());
        let dpms = sys::find_edp_dpms().and_then(|p| Attr::open(p, false).ok());

        let d = Daemon {
            als,
            screen,
            screen_notify,
            kbd,
            power_check: battery.as_ref().map(|_| Instant::now()),
            battery,
            uevents,
            keys,
            dpms,
            verbose,
            dry_run,
            filter: LuxFilter::default(),
            lux: 0.0,
            on_ac: false,
            panel_off: false,
            asleep: sys::time_asleep(),
            hold_until: None,
            saved: None,
        };
        if verbose {
            let path = |a: &Option<Attr>| a.as_ref().map_or("(none)".into(), |a| a.path.display().to_string());
            say!("autobrightd: als {}", d.als.path.display());
            say!("autobrightd: screen {} max {}", d.screen.attr.path.display(), d.screen.channel.settings.max);
            match &d.screen.channel.map {
                Some(map) => say!("autobrightd: panel map: {} points", map.len()),
                None => say!("autobrightd: no panel map (autobrightd --calibrate makes one)"),
            }
            match &d.kbd {
                Some(k) => say!("autobrightd: kbd {} max {}", k.attr.path.display(), k.channel.settings.max),
                None => say!("autobrightd: kbd (none)"),
            }
            let how = if d.uevents.is_some() { "on uevents" } else { "polled" };
            say!("autobrightd: power {} ({how})", path(&d.battery));
            say!("autobrightd: dpms {}, notify {}", path(&d.dpms), path(&d.screen_notify));
            if dry_run {
                say!("autobrightd: dry run, nothing is written");
            }
        }
        Ok(d)
    }

    fn log(&self, msg: std::fmt::Arguments) {
        if self.verbose {
            say!("autobrightd: {msg}");
        }
    }

    fn backlights(&mut self) -> impl Iterator<Item = &mut Backlight> {
        std::iter::once(&mut self.screen).chain(self.kbd.as_mut())
    }

    /// Reads a backlight and notes a change made by someone else.
    fn check(&mut self, which: Which) {
        if self.dry_run {
            return;
        }
        let lux = self.lux;
        let verbose = self.verbose;
        let Some(b) = self.get(which) else { return };
        if let Ok(v) = b.attr.read_u32()
            && b.channel.observe(v, lux)
            && verbose
        {
            say!("autobrightd: {} set to {v} by someone else, holding until the light changes", b.channel.name);
        }
    }

    fn get(&mut self, which: Which) -> Option<&mut Backlight> {
        match which {
            Which::Screen => Some(&mut self.screen),
            Which::Kbd => self.kbd.as_mut(),
        }
    }

    /// Reads the battery status: anything but Discharging means the charger
    /// is connected. The next sample applies the new AC boost.
    fn check_power(&mut self) {
        let Some(battery) = &self.battery else { return };
        let mut buf = [0; 32];
        let status = match battery.read_str(&mut buf) {
            Ok(s) => s.to_owned(),
            Err(e) => {
                self.log(format_args!("reading {}: {e}", battery.path.display()));
                return;
            }
        };
        let on = status != "Discharging";
        if on != self.on_ac {
            self.on_ac = on;
            self.log(format_args!("{} (battery {status})", if on { "on AC" } else { "on battery" }));
        }
    }

    /// One sensor sample: DPMS, lux, keyboard check, new targets.
    fn sample(&mut self, now: Instant) {
        let off = self.dpms.as_ref().is_some_and(|d| {
            let mut buf = [0; 32];
            d.read_str(&mut buf).is_ok_and(|s| s != "On")
        });
        if off != self.panel_off {
            self.panel_off = off;
            self.log(format_args!("panel {}", if off { "off" } else { "on" }));
            if off {
                self.panel_turned_off();
            }
        }
        if off {
            return;
        }

        let raw = match self.als.read_int() {
            Ok(v) => v as f32,
            Err(e) => {
                self.log(format_args!("reading lux: {e}"));
                return;
            }
        };
        self.lux = self.filter.update(raw);
        // The LED class sends no change notification, so look each sample.
        self.check(Which::Kbd);
        // Just woken: the lid is still opening, the sensor in its shadow.
        if self.hold_until.is_some_and(|t| now < t) {
            return;
        }
        self.hold_until = None;

        let lux = self.lux;
        let boost = if self.on_ac { logic::AC_SCREEN_BOOST_PCT } else { 0.0 };
        let verbose = self.verbose;
        for b in self.backlights() {
            let boost = if b.channel.name == "screen" { boost } else { 0.0 };
            let update = b.channel.update(lux, boost, now);
            if !verbose {
                continue;
            }
            if update == Update::Resumed {
                say!("autobrightd: {}: light changed (lux {lux:.0}), automatic again", b.channel.name);
            }
            if let Update::Moving(to) = update {
                say!(
                    "autobrightd: lux {raw:.0} (smoothed {lux:.0}): {} {} -> {to}",
                    b.channel.name, b.channel.known
                );
            }
        }
    }

    /// Panel blanked: switch the keyboard off at once and stop all ramps.
    /// Screen writes would only wake the DCP for nothing; the kernel keeps
    /// the value for when the panel comes back.
    fn panel_turned_off(&mut self) {
        self.filter.reset();
        self.screen.channel.stop();
        let dry_run = self.dry_run;
        if let Some(k) = &mut self.kbd {
            k.channel.stop();
            k.channel.override_lux = None;
            if k.channel.known > 0 && (dry_run || k.attr.write_u32(0).is_ok()) {
                k.channel.wrote(0);
                k.channel.stop();
            }
            // Nothing by hand is left to go back to after a sleep, either.
            k.channel.keep();
        }
    }

    /// The machine slept. Closing the lid darkened the sensor before that,
    /// which may have started dimming the screen or ended a manual setting:
    /// both backlights go back, at once, to how they were set before
    /// (logic::SLEEP_LEAD), and the light counts again from scratch after
    /// logic::WAKE_HOLD.
    fn woke(&mut self, slept: Duration, now: Instant) {
        self.filter.reset();
        self.hold_until = Some(now + logic::WAKE_HOLD);
        // The charger may have been plugged in or pulled while asleep.
        if self.battery.is_some() {
            self.power_check = Some(now);
        }
        let (dry_run, verbose) = (self.dry_run, self.verbose);
        if verbose {
            say!("autobrightd: slept {:.0} s", slept.as_secs_f32());
        }
        for b in self.backlights() {
            // Whatever the file holds now is not a change by hand.
            let current = if dry_run { b.channel.known } else { b.attr.read_u32().unwrap_or(b.channel.known) };
            let to = b.channel.woke(current, now);
            set_now(b, to, dry_run);
            if verbose {
                let kept = b.channel.kept(now);
                say!("autobrightd: {} {current} -> {to} as before the sleep ({})", b.channel.name, mode(kept));
            }
        }
    }

    /// Saves what the backlights are set to, when that has changed, for the
    /// next start.
    fn save(&mut self, now: Instant) {
        // A brightness key that is down moves its target with every frame.
        let key_down = |b: &mut Backlight| b.channel.ramp.as_ref().is_some_and(|r| r.by_key);
        if self.dry_run || self.backlights().any(key_down) {
            return;
        }
        let kept = (self.screen.channel.kept(now), self.kbd.as_mut().map(|k| k.channel.kept(now)));
        if self.saved == Some(kept) {
            return;
        }
        // Even if it fails: say so once, not five times a second.
        self.saved = Some(kept);
        let entry = |b: &Backlight, kept| state::Entry {
            name: b.channel.name.to_owned(),
            max: b.channel.settings.max,
            kept,
        };
        let mut entries = vec![entry(&self.screen, kept.0)];
        entries.extend(self.kbd.as_ref().zip(kept.1).map(|(k, kept)| entry(k, kept)));
        if let Err(e) = state::save(&entries) {
            say!("autobrightd: {e}");
        }
    }

    /// Writes ramp frames that are due.
    fn frames(&mut self, now: Instant) {
        for which in [Which::Screen, Which::Kbd] {
            let due = self.get(which).and_then(|b| b.channel.next_frame()).is_some_and(|t| t <= now);
            if !due {
                continue;
            }
            // Someone may have changed it since our last write.
            self.check(which);
            let (dry_run, verbose, lux) = (self.dry_run, self.verbose, self.lux);
            let Some(b) = self.get(which) else { continue };
            let Some(value) = b.channel.frame(now) else { continue };
            if dry_run {
                b.channel.wrote(value);
                if verbose {
                    say!("autobrightd: would write {} {value}", b.channel.name);
                }
                continue;
            }
            let started = Instant::now();
            match b.attr.write_u32(value) {
                Ok(()) => {
                    b.channel.wrote(value);
                    // Without the kernel patches each write holds the
                    // backlight lock for about 45 ms, and a write by someone
                    // else that waited for it may already have replaced our
                    // value; if so, that is a manual change.
                    let stored = b.attr.read_u32().unwrap_or(value);
                    b.channel.not_before(Instant::now() + logic::WRITE_GAP);
                    if verbose {
                        let ms = started.elapsed().as_millis();
                        say!("autobrightd: {} {value} ({ms} ms)", b.channel.name);
                    }
                    if stored != value && b.channel.observe(stored, lux) && verbose {
                        say!(
                            "autobrightd: {} set to {stored} by someone else, holding until the light changes",
                            b.channel.name
                        );
                    }
                }
                Err(e) => {
                    say!("autobrightd: writing {}: {e}", b.attr.path.display());
                    b.channel.stop();
                }
            }
        }
    }

    fn run(&mut self) -> ! {
        let mut next_sample = Instant::now();
        loop {
            let now = Instant::now();
            let asleep = sys::time_asleep();
            if asleep >= self.asleep + SLEEP_MIN {
                self.woke(asleep - self.asleep, now);
                self.asleep = asleep;
                next_sample = now;
            }
            let mut deadline = next_sample;
            if !self.panel_off {
                for b in self.backlights() {
                    if let Some(t) = b.channel.next_frame() {
                        deadline = deadline.min(t);
                    }
                }
            }
            if let Some(t) = self.power_check {
                deadline = deadline.min(t);
            }
            let notify = self.screen_notify.as_ref().map(Attr::fd);
            let uevent = self.uevents.as_ref().map(Uevents::fd);
            let keys = self.keys.as_ref().map(KeySignals::fd);
            let ready = match sys::wait(notify, uevent, keys, deadline.saturating_duration_since(now)) {
                Ok(r) => r,
                Err(e) => {
                    say!("autobrightd: poll: {e}");
                    exit(1);
                }
            };
            if ready.keys {
                let events = self.keys.as_ref().map_or_else(Vec::new, KeySignals::drain);
                for event in events {
                    match event {
                        KeyEvent::Press(presses) => {
                            let to = self.screen.channel.key_step(presses, self.lux, Instant::now());
                            self.log(format_args!("brightness key {presses:+}: screen -> {to}"));
                        }
                        KeyEvent::Release => {
                            self.screen.channel.key_release();
                            let to = self.screen.channel.target;
                            self.log(format_args!("brightness key let go: screen -> {to}"));
                        }
                    }
                }
            }
            if ready.notify {
                // Re-arm, then see whether it was us.
                if let Some(n) = &self.screen_notify {
                    let _ = n.read_int();
                }
                self.check(Which::Screen);
            }

            let now = Instant::now();
            if ready.uevent && self.uevents.as_mut().is_some_and(|u| u.drain(POWER_SUPPLY_PREFIX)) {
                self.check_power();
                self.power_check = Some(now + POWER_RECHECK);
            }
            if self.power_check.is_some_and(|t| t <= now) {
                self.check_power();
                self.power_check = if self.uevents.is_some() { None } else { Some(now + POWER_POLL) };
            }
            if now >= next_sample {
                self.sample(now);
                self.save(now);
                next_sample = if self.panel_off {
                    now + DPMS_OFF_CHECK
                } else if next_sample + SAMPLE > now {
                    next_sample + SAMPLE
                } else {
                    now + SAMPLE
                };
            }
            if !self.panel_off {
                self.frames(Instant::now());
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Which {
    Screen,
    Kbd,
}

fn main() {
    let mut verbose = false;
    let mut dry_run = false;
    let mut calibrate = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" => verbose = true,
            "-n" => dry_run = true,
            "--calibrate" => calibrate = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => {
                say!("{USAGE}");
                exit(2);
            }
        }
    }
    // Before anything else: until the handlers are in, a key press (a
    // SIGUSR1 or SIGUSR2) would end the process.
    let keys = match KeySignals::install() {
        Ok(k) => Some(k),
        Err(e) => {
            say!("autobrightd: brightness keys: {e}");
            None
        }
    };
    if calibrate {
        // The key handlers stay in, so a brightness key does not end it.
        if let Err(e) = calibrate::run(Path::new(SCREEN_DIR)) {
            say!("autobrightd: {e}");
            exit(1);
        }
        return;
    }
    match Daemon::open(verbose, dry_run, keys) {
        Ok(mut d) => d.run(),
        Err(e) => {
            say!("autobrightd: {e}");
            exit(1);
        }
    }
}
