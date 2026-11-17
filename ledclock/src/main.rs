// SPDX-License-Identifier: GPL-3.0-or-later
//! ledclock: a clock on the desktop, drawn as a red LED display in a metal
//! frame.
//!
//! The window has no title bar, is on every workspace, takes no focus and
//! stays under the other windows (or over them with -above). The frame and
//! the panel are computed for the size of the window (src/face.rs), and so
//! are the digits, of seven segments each (src/segments.rs): the clock is
//! as sharp at Window Maker's GUI scale factor as at 1. AM and PM are
//! drawn by Xft, and so is the charge of the battery, in percent, over
//! them.
//!
//! The pointer moves the clock, and makes it larger or smaller by a
//! corner, in its proportions. Window Maker does neither for a window of
//! this kind, so ledclock follows the pointer itself, and it keeps the
//! place and the width for the next time it starts.
//!
//! It wakes once a minute, at the minute, on a timer that follows the wall
//! clock: the time is right again at once after the machine was suspended
//! or the clock was set. Nothing else wakes it but a pointer that has hold
//! of the clock: the picture is the window's background, which the X
//! server puts back by itself, and the X server shows the cursor of a
//! corner by itself, over a window of the corner's own. Each minute only
//! the digits that changed are drawn again.

mod face;
mod segments;
mod xlib;

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong};
use std::path::PathBuf;
use std::{env, fs, mem, process, ptr};

use xlib::*;

const USAGE: &str =
    "usage: ledclock [-24] [-above] [-geometry +X+Y] [-width PIXELS] [-color RRGGBB] [-font FONT]";
/// The LEDs of the design.
const DEFAULT_COLOR: u32 = 0xff0000;
/// AM and PM are letters, as the design has them; without that font, the
/// screen's monospace one. The digits are not in a font (src/segments.rs).
const DEFAULT_FONT: &str = "DejaVu Sans Mono,monospace";
/// In pixels of the design (src/face.rs): the height of the letters of AM,
/// the line they and the digits stand on, and the space between AM and the
/// digits.
const MARKER_HEIGHT: f64 = 17.0;
const BASELINE: f64 = 63.0;
const MARKER_GAP: f64 = 12.0;
/// How much of the colour a segment has that is not lit: it shows, as on a
/// display that is off.
const UNLIT: f64 = 0.12;
/// Where the batteries are.
const POWER_SUPPLIES: &str = "/sys/class/power_supply";
/// What the eye takes for the margins on the two sides of the time, in
/// pixels of the design. The frame's shadow lies on the left of the panel,
/// which looks as if it began this much further in.
const SHADOW: f64 = 3.0;
/// The charge of the battery is a longer line than AM under it, and the
/// eye puts the left edge of the two between their ends: this much of the
/// way from the end of the longer one.
const RAGGED: f64 = 0.3;
/// The range WINGs accepts for GSScaleFactor.
const MIN_FACTOR: f64 = 0.5;
const MAX_FACTOR: f64 = 8.0;
const MIN_WIDTH: u32 = 60;
const MAX_WIDTH: u32 = 8192;
/// How far a corner reaches into the window, for the pointer: in pixels of
/// the design, and at least so many of the screen.
const CORNER: f64 = 20.0;
const MIN_CORNER: i32 = 8;

fn die(msg: &str) -> ! {
    eprintln!("ledclock: {msg}");
    process::exit(1)
}

/// One coordinate of -geometry: from the left or top edge of the screen, or
/// from the right or bottom one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Offset {
    Start(i32),
    End(i32),
}

impl Offset {
    /// The position of a window `size` pixels long on a screen of `screen`.
    fn resolve(self, screen: i32, size: i32) -> i32 {
        match self {
            Offset::Start(n) => n,
            Offset::End(n) => screen - size - n,
        }
    }
}

struct Options {
    hours24: bool,
    above: bool,
    position: Option<(Offset, Offset)>,
    width: Option<u32>,
    color: u32,
    font: String,
}

/// `+X+Y`, as X programs take it: `-X` and `-Y` count from the right and
/// the bottom edge.
fn parse_position(s: &str) -> Option<(Offset, Offset)> {
    let s = s.strip_prefix('=').unwrap_or(s);
    let second = s.get(1..)?.find(['+', '-'])? + 1;
    let offset = |part: &str| {
        let n: i32 = part.get(1..)?.parse().ok().filter(|n| *n >= 0)?;
        match part.as_bytes()[0] {
            b'+' => Some(Offset::Start(n)),
            b'-' => Some(Offset::End(n)),
            _ => None,
        }
    };
    Some((offset(&s[..second])?, offset(&s[second..])?))
}

/// `RRGGBB` or `#RRGGBB`.
fn parse_color(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn parse_args() -> Options {
    let mut opts = Options {
        hours24: false,
        above: false,
        position: None,
        width: None,
        color: DEFAULT_COLOR,
        font: DEFAULT_FONT.to_string(),
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-24" => opts.hours24 = true,
            "-above" => opts.above = true,
            "-geometry" => match args.next().as_deref().and_then(parse_position) {
                Some(p) => opts.position = Some(p),
                None => die("-geometry takes a position: +X+Y, -X+Y, +X-Y or -X-Y"),
            },
            "-width" => match args.next().and_then(|s| s.parse().ok()) {
                Some(w) if (MIN_WIDTH..=MAX_WIDTH).contains(&w) => opts.width = Some(w),
                _ => die(&format!("-width takes {MIN_WIDTH} to {MAX_WIDTH} pixels")),
            },
            "-color" => match args.next().as_deref().and_then(parse_color) {
                Some(c) => opts.color = c,
                None => die("-color takes RRGGBB"),
            },
            "-font" => match args.next() {
                Some(f) if !f.is_empty() => opts.font = f,
                _ => die(USAGE),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                process::exit(0);
            }
            _ => die(USAGE),
        }
    }
    opts
}

fn parse_factor(s: &str) -> Option<f64> {
    match s.trim().parse::<f64>() {
        Ok(f) if f > 0.0 && f.is_finite() => Some(f.clamp(MIN_FACTOR, MAX_FACTOR)),
        _ => None,
    }
}

/// GSScaleFactor from the WMGLOBAL defaults domain, where WINGs reads it
/// (the same reading as scd's).
fn wmglobal_factor() -> Option<f64> {
    let home = env::var("HOME").unwrap_or_default();
    let root = match env::var("WMAKER_USER_ROOT") {
        Ok(dir) if !dir.is_empty() => match dir.strip_prefix("~/") {
            Some(rest) => format!("{home}/{rest}"),
            None => dir,
        },
        _ => format!("{home}/GNUstep"),
    };
    let text = fs::read_to_string(format!("{root}/Defaults/WMGLOBAL")).ok()?;
    // { GSScaleFactor = 2; } or { "GSScaleFactor" = "1.5"; }
    let i = text.find("GSScaleFactor")?;
    let rest = text[i + "GSScaleFactor".len()..].trim_start_matches('"').trim_start();
    let rest = rest.strip_prefix('=')?.trim_start().trim_start_matches('"');
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    parse_factor(&rest[..end])
}

/// The factor Window Maker uses: GS_SCALE_FACTOR, then GSScaleFactor.
fn gui_factor() -> f64 {
    env::var("GS_SCALE_FACTOR").ok().and_then(|s| parse_factor(&s)).or_else(wmglobal_factor).unwrap_or(1.0)
}

/// The window for a width: the height keeps the proportions of the design.
fn window_size(width: u32) -> (i32, i32) {
    (width as i32, ((width as f64 * face::HEIGHT / face::WIDTH).round() as i32).max(1))
}

/// What the clock shows at `hour`:`minute`: AM or PM, unless the clock
/// counts 24 hours, and the four digits.
fn clock_text(hour: i32, minute: i32, hours24: bool) -> (Option<&'static str>, [u8; 4]) {
    let digits = |hour: i32| [hour / 10, hour % 10, minute / 10, minute % 10].map(|d| d as u8);
    if hours24 {
        return (None, digits(hour));
    }
    let marker = if hour < 12 { "AM" } else { "PM" };
    let hour = match hour % 12 {
        0 => 12,
        h => h,
    };
    (Some(marker), digits(hour))
}

/// The charge of a power supply, in percent, from what its `type`, `scope`
/// and `capacity` files say: of a battery, and not of the battery of a
/// mouse or a keyboard.
fn battery_charge(kind: Option<&str>, scope: Option<&str>, capacity: Option<&str>) -> Option<u8> {
    if kind? != "Battery" || scope == Some("Device") {
        return None;
    }
    capacity?.parse::<u8>().ok().map(|percent| percent.min(100))
}

/// The machine's battery: its `capacity` file, once it was found among the
/// power supplies.
#[derive(Default)]
struct Battery {
    capacity: Option<PathBuf>,
}

impl Battery {
    /// The charge of the battery, in percent; None on a machine without
    /// one. Only its own file is read, after the first time: the power
    /// supplies are looked through again when that file cannot be read.
    fn charge(&mut self) -> Option<u8> {
        let known = self.capacity.as_ref().and_then(|path| fs::read_to_string(path).ok());
        if let Some(percent) = known.as_deref().and_then(|s| s.trim().parse::<u8>().ok()) {
            return Some(percent.min(100));
        }
        let (capacity, percent) = Battery::find()?;
        self.capacity = Some(capacity);
        Some(percent)
    }

    /// The capacity file of the machine's battery, and what it says.
    fn find() -> Option<(PathBuf, u8)> {
        let mut supplies: Vec<PathBuf> =
            fs::read_dir(POWER_SUPPLIES).ok()?.flatten().map(|entry| entry.path()).collect();
        supplies.sort();
        supplies.iter().find_map(|supply| {
            let read = |name: &str| fs::read_to_string(supply.join(name)).ok();
            let (kind, scope, capacity) = (read("type"), read("scope"), read("capacity"));
            fn trimmed(s: &Option<String>) -> Option<&str> {
                s.as_deref().map(str::trim)
            }
            let percent = battery_charge(trimmed(&kind), trimmed(&scope), trimmed(&capacity))?;
            Some((supply.join("capacity"), percent))
        })
    }
}

/// How far inside its column of letters the eye sees the left edge of
/// them. `longer` and `shorter` are the widths of the two lines, the
/// charge of the battery and AM under it, and `bearing` is the empty part
/// of the first letter's width, on its left.
fn inset(longer: i32, shorter: i32, bearing: i32) -> f64 {
    let ragged = if shorter > 0 { RAGGED * (longer - shorter) as f64 } else { 0.0 };
    bearing as f64 + ragged
}

/// Where the letters and the digits start, from the left of a window
/// `width` pixels wide: the two together are in the middle of the panel,
/// with margins that look the same on both sides. `letters` is the width
/// of the column of letters on the left of the digits, AM and the charge
/// of the battery, 0 without them; `inset` is how far inside that column
/// the letters are seen to begin.
fn text_origins(width: i32, letters: i32, inset: f64) -> (i32, i32) {
    let scale = width as f64 / face::WIDTH;
    let gap = if letters > 0 { (MARKER_GAP * scale).round() as i32 } else { 0 };
    let total = letters + gap + (segments::WIDTH * scale).round() as i32;
    let middle = (face::PANEL_LEFT + SHADOW + face::PANEL_RIGHT) / 2.0 * scale;
    let start = (middle - (inset + total as f64) / 2.0).round() as i32;
    (start, start + letters + gap)
}

fn atom(dpy: *mut Display, name: &str) -> Atom {
    let name = CString::new(name).unwrap();
    // SAFETY: valid display and NUL-terminated name.
    unsafe { XInternAtom(dpy, name.as_ptr(), 0) }
}

fn set_longs(dpy: *mut Display, win: Window, prop: Atom, ty: Atom, data: &[c_long]) {
    // SAFETY: format 32 data is an array of longs for Xlib.
    unsafe {
        XChangeProperty(dpy, win, prop, ty, 32, PROP_MODE_REPLACE, data.as_ptr() as *const c_uchar, data.len() as c_int)
    };
}

fn set_bytes(dpy: *mut Display, win: Window, prop: Atom, ty: Atom, data: &[u8]) {
    // SAFETY: format 8 data of the given length.
    unsafe { XChangeProperty(dpy, win, prop, ty, 8, PROP_MODE_REPLACE, data.as_ptr(), data.len() as c_int) };
}

/// How far left a colour of 8 bits goes in a pixel with this mask.
fn mask_shift(mask: c_ulong) -> u32 {
    if mask == 0 || mask >> mask.trailing_zeros() != 0xff {
        die("the screen needs a TrueColor visual with 8 bits per colour");
    }
    mask.trailing_zeros()
}

/// What the pointer takes hold of when the button goes down: the clock, to
/// move it, or one of its corners, to make it larger or smaller.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    Body,
    Corner { left: bool, top: bool },
}

/// How far a corner reaches into a window `width` pixels wide, across and
/// down.
fn corner_reach(width: i32) -> i32 {
    ((CORNER * width as f64 / face::WIDTH).round() as i32).max(MIN_CORNER)
}

/// What is under the pointer at `x`, `y` of a window of this size.
fn grip_at(width: i32, height: i32, x: i32, y: i32) -> Grip {
    let reach = corner_reach(width);
    let (left, right) = (x < reach, x >= width - reach);
    let (top, bottom) = (y < reach, y >= height - reach);
    if (left || right) && (top || bottom) { Grip::Corner { left, top } } else { Grip::Body }
}

/// The place of a corner in `Clock::corners` and `CORNERS`.
fn corner_cursor(left: bool, top: bool) -> usize {
    match (top, left) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

/// The four corners, left and top, in the order of `corner_cursor`.
const CORNERS: [(bool, bool); 4] = [(true, true), (false, true), (true, false), (false, false)];

/// The square of a corner in a window of this size, where the pointer
/// takes hold of the corner: x, y and the length of its side.
fn corner_square(width: i32, height: i32, left: bool, top: bool) -> (i32, i32, i32) {
    let reach = corner_reach(width);
    (if left { 0 } else { width - reach }, if top { 0 } else { height - reach }, reach)
}

/// The position nearest to `at` where a window `size` pixels long is whole
/// on a screen of `screen`.
fn on_screen(at: i32, size: i32, screen: i32) -> i32 {
    at.clamp(0, (screen - size).max(0))
}

/// A window the pointer has hold of: by what, and where the pointer and
/// the window were when the button went down.
struct Drag {
    grip: Grip,
    pointer: (i32, i32),
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Drag {
    /// Where the window is and how wide (x, y, width) with the pointer at
    /// `pointer`. Moved, it stays whole on the screen. By a corner, it
    /// keeps its proportions and the opposite corner stays where it is: the
    /// corner follows the pointer in the direction, across or down, in
    /// which the pointer has changed the size more.
    fn placement(&self, pointer: (i32, i32), screen: (i32, i32)) -> (i32, i32, i32) {
        let (dx, dy) = (pointer.0 - self.pointer.0, pointer.1 - self.pointer.1);
        match self.grip {
            Grip::Body => {
                (on_screen(self.x + dx, self.width, screen.0), on_screen(self.y + dy, self.height, screen.1), self.width)
            }
            Grip::Corner { left, top } => {
                let wider = if left { -dx } else { dx };
                let taller = if top { -dy } else { dy };
                let by_width = self.width + wider;
                let by_height = ((self.height + taller) as f64 * face::WIDTH / face::HEIGHT).round() as i32;
                let across = (by_width - self.width).abs() >= (by_height - self.width).abs();
                let width = if across { by_width } else { by_height };
                let width = width.clamp(MIN_WIDTH as i32, MAX_WIDTH as i32);
                let (width, height) = window_size(width as u32);
                let x = if left { self.x + self.width - width } else { self.x };
                let y = if top { self.y + self.height - height } else { self.y };
                (x, y, width)
            }
        }
    }
}

/// Where the placement the pointer gave the window is kept:
/// $XDG_STATE_HOME/ledclock/placement.
fn placement_file() -> Option<PathBuf> {
    let base = match env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(env::var_os("HOME")?).join(".local/state"),
    };
    Some(base.join("ledclock/placement"))
}

/// `X Y WIDTH`, as it is saved.
fn parse_placement(s: &str) -> Option<(i32, i32, i32)> {
    let mut numbers = s.split_whitespace().map(|n| n.parse::<i32>().ok());
    let placement = (numbers.next()??, numbers.next()??, numbers.next()??);
    let width = (MIN_WIDTH as i32..=MAX_WIDTH as i32).contains(&placement.2);
    (width && numbers.next().is_none()).then_some(placement)
}

fn saved_placement() -> Option<(i32, i32, i32)> {
    parse_placement(&fs::read_to_string(placement_file()?).ok()?)
}

fn save_placement((x, y, width): (i32, i32, i32)) {
    let Some(path) = placement_file() else { return };
    let written = match path.parent() {
        Some(dir) => fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|()| fs::write(&path, format!("{x} {y} {width}\n")));
    if let Err(e) = written {
        eprintln!("ledclock: cannot save the placement in {}: {e}", path.display());
    }
}

/// Where the window is and how wide (x, y, width) when ledclock starts.
/// -geometry and -width come first, then the placement the pointer gave it
/// the last time, kept on a screen that may be smaller now; without either,
/// the size of the design at the GUI factor, in the middle of the screen's
/// width, half its own height from the top.
fn first_placement(
    opts: &Options,
    saved: Option<(i32, i32, i32)>,
    factor: f64,
    screen: (i32, i32),
) -> (i32, i32, i32) {
    let width = match (opts.width, saved) {
        (Some(width), _) => width,
        (None, Some((_, _, width))) => width as u32,
        (None, None) => (face::WIDTH * factor).round() as u32,
    };
    let (width, height) = window_size(width);
    let (x, y) = match (opts.position, saved) {
        (Some((x, y)), _) => (x.resolve(screen.0, width), y.resolve(screen.1, height)),
        (None, Some((x, y, _))) => (on_screen(x, width, screen.0), on_screen(y, height, screen.1)),
        (None, None) => ((screen.0 - width) / 2, height / 2),
    };
    (x, y, width)
}

/// What the clock shows: AM or PM, the four digits, and the charge of the
/// battery.
type Text = (Option<&'static str>, [u8; 4], Option<u8>);

/// A part of the window: x, y, width and height.
type Area = (i32, i32, i32, i32);

struct Clock {
    dpy: *mut Display,
    screen: c_int,
    visual: *mut Visual,
    depth: c_int,
    win: Window,
    gc: *mut Gc,
    draw: *mut XftDraw,
    fg: XftColor,
    /// The colour of the LEDs: red, green and blue.
    led: [f64; 3],
    /// The font of AM and PM, as -font names it, and the height of an M
    /// in it at 100 pixels.
    family: String,
    sample: f64,
    hours24: bool,
    /// The cursor over the clock while it is moved, and the ones over its
    /// corners (`corner_cursor`).
    fleur: Cursor,
    corners: [Cursor; 4],
    /// Windows that take no input of their own and only show a cursor,
    /// over the four corners (`corner_square`): the X server shows the
    /// cursor of a corner by itself, and the pointer does not wake ledclock
    /// on its way over the clock.
    squares: [Window; 4],
    /// Where the window was put, on the screen.
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    // What follows is made for the size of the window (`fit`).
    /// The edges of the frame are not smoothed: the pointer is changing the
    /// size, and the picture is computed at every size on the way.
    rough: bool,
    /// The frame and the panel: the colour of every pixel, row by row, and
    /// the same on a pixmap.
    colors: Vec<[u8; 3]>,
    back: Pixmap,
    /// The same with the time on it: the background of the window, which
    /// the X server puts back by itself where the window is uncovered.
    pixmap: Pixmap,
    /// What is on `pixmap`, and where its digits start; None when there is
    /// only the frame and the panel.
    drawn: Option<Text>,
    digits_x: i32,
    marker: *mut XftFont,
    /// The width of AM or PM; 0 on a clock of 24 hours.
    marker_width: i32,
    /// The empty part of the width of a digit of the charge, on its left.
    bearing: i32,
    /// The line the digits and AM stand on, and the one above it, where
    /// the letters reach as high as the digits: the charge of the battery
    /// is there when AM is under it.
    baseline: i32,
    upper: i32,
}

impl Clock {
    fn advance(dpy: *mut Display, font: *mut XftFont, s: &str) -> i32 {
        let mut g = XGlyphInfo::default();
        // SAFETY: valid display and font; `s` is UTF-8 of the given length.
        unsafe { XftTextExtentsUtf8(dpy, font, s.as_ptr(), s.len() as c_int, &mut g) };
        g.x_off as i32
    }

    /// The empty part of the width of the first letter of `s`, on its left.
    fn bearing(dpy: *mut Display, font: *mut XftFont, s: &str) -> i32 {
        let mut g = XGlyphInfo::default();
        // SAFETY: valid display and font; `s` is UTF-8 of the given length.
        unsafe { XftTextExtentsUtf8(dpy, font, s.as_ptr(), s.len() as c_int, &mut g) };
        -(g.x as i32)
    }

    /// The font `family` at `size` pixels.
    fn open_font(dpy: *mut Display, screen: c_int, family: &str, size: f64) -> *mut XftFont {
        // Grey antialiasing: the colour fringes of subpixel rendering show
        // on red letters.
        let pattern = format!("{family}:pixelsize={size:.2}:antialias=true:rgba=none");
        let name = CString::new(pattern.clone()).unwrap_or_else(|_| die("bad font name"));
        // SAFETY: valid display and NUL-terminated name.
        let font = unsafe { XftFontOpenName(dpy, screen, name.as_ptr()) };
        if font.is_null() {
            die(&format!("cannot open the font `{pattern}`"));
        }
        font
    }

    /// The height of `sample` in the font `family` at 100 pixels: once,
    /// for every size the window will have.
    fn measure(dpy: *mut Display, screen: c_int, family: &str, sample: &str) -> f64 {
        let trial = Clock::open_font(dpy, screen, family, 100.0);
        let mut g = XGlyphInfo::default();
        // SAFETY: valid display and font; the font is not used after it is
        // closed.
        unsafe {
            XftTextExtentsUtf8(dpy, trial, sample.as_ptr(), sample.len() as c_int, &mut g);
            XftFontClose(dpy, trial);
        }
        if g.height == 0 {
            die(&format!("the font `{family}` has no `{sample}`"));
        }
        g.height as f64
    }

    /// Computes the frame and the panel and puts them on `self.back`, and
    /// draws on them again what was on the window. `smooth` takes nine
    /// times as long at a size that needs it.
    fn put_face(&mut self, smooth: bool) {
        let (w, h) = (self.width as usize, self.height as usize);
        // At a whole multiple of the design every edge of it falls between
        // two pixels; at another size the edges are smoothed.
        let whole = (self.width as f64 / face::WIDTH).fract() == 0.0;
        self.rough = !whole && !smooth;
        self.colors = face::render(w, h, if whole || !smooth { 1 } else { 3 });
        self.put_image(self.back, &self.colors, 0, 0, w, h);
        if let Some(text) = self.drawn.take() {
            self.draw(text);
        }
    }

    /// Puts a picture `w` pixels wide and `h` high, row by row in `colors`,
    /// at `x`, `y` of a pixmap.
    fn put_image(&self, to: Pixmap, colors: &[[u8; 3]], x: usize, y: usize, w: usize, h: usize) {
        // SAFETY: the image is created without data, given `pixels` for the
        // time of XPutImage, and freed without it.
        unsafe {
            let image = XCreateImage(
                self.dpy,
                self.visual,
                self.depth as c_uint,
                Z_PIXMAP,
                0,
                ptr::null_mut(),
                w as c_uint,
                h as c_uint,
                32,
                0,
            );
            if image.is_null() || (*image).bits_per_pixel != 32 {
                die("the screen needs pixels of 32 bits");
            }
            let shifts = [(*image).red_mask, (*image).green_mask, (*image).blue_mask].map(mask_shift);
            let mut pixels: Vec<u32> = colors
                .iter()
                .map(|c| (c[0] as u32) << shifts[0] | (c[1] as u32) << shifts[1] | (c[2] as u32) << shifts[2])
                .collect();
            // The pixels are in this machine's byte order; Xlib swaps them
            // for a server of the other one.
            (*image).byte_order = if cfg!(target_endian = "big") { MSB_FIRST } else { LSB_FIRST };
            (*image).data = pixels.as_mut_ptr() as *mut c_char;
            XPutImage(self.dpy, to, self.gc, image, 0, 0, x as c_int, y as c_int, w as c_uint, h as c_uint);
            (*image).data = ptr::null_mut();
            XDestroyImage(image);
        }
    }

    /// Makes the pixmaps, the font and the picture of the frame for a
    /// window of this size, in the place of the ones for the size before,
    /// with what was on the window drawn on them again.
    fn fit(&mut self, width: i32, height: i32, smooth: bool) {
        let (dpy, screen) = (self.dpy, self.screen);
        let scale = height as f64 / face::HEIGHT;
        // SAFETY: the pixmaps and the font of the size before are freed
        // once, after the first call only, and replaced before anything
        // draws. The X server keeps the pixmap that is still the window's
        // background until the window is given the new one.
        unsafe {
            if !self.marker.is_null() {
                XftFontClose(dpy, self.marker);
                XFreePixmap(dpy, self.back);
                XFreePixmap(dpy, self.pixmap);
            }
            self.back = XCreatePixmap(dpy, self.win, width as c_uint, height as c_uint, self.depth as c_uint);
            self.pixmap = XCreatePixmap(dpy, self.win, width as c_uint, height as c_uint, self.depth as c_uint);
            XftDrawChange(self.draw, self.pixmap);
            for (&square, &(left, top)) in self.squares.iter().zip(&CORNERS) {
                let (x, y, side) = corner_square(width, height, left, top);
                XMoveResizeWindow(dpy, square, x, y, side as c_uint, side as c_uint);
            }
        }
        (self.width, self.height) = (width, height);
        // A letter that is flat at both ends gives the height.
        self.marker = Clock::open_font(dpy, screen, &self.family, 100.0 * (MARKER_HEIGHT * scale) / self.sample);
        self.marker_width = if self.hours24 {
            0
        } else {
            Clock::advance(dpy, self.marker, "AM").max(Clock::advance(dpy, self.marker, "PM"))
        };
        // Of the same digit for every charge: the time does not move by a
        // pixel when the charge goes from 70 to 69.
        self.bearing = Clock::bearing(dpy, self.marker, "0");
        self.baseline = (BASELINE * scale).round() as i32;
        self.upper = ((BASELINE - segments::HEIGHT + MARKER_HEIGHT) * scale).round() as i32;
        self.put_face(smooth);
    }

    /// Tells the window manager where the window was put and that its size
    /// is the only one it has: USPosition | PPosition | PMinSize | PMaxSize.
    fn set_size_hints(&self) {
        let (w, h) = (self.width as c_long, self.height as c_long);
        let mut size = [0 as c_long; 18];
        size[..9].copy_from_slice(&[1 | 4 | 16 | 32, self.x as c_long, self.y as c_long, w, h, w, h, w, h]);
        set_longs(self.dpy, self.win, XA_WM_NORMAL_HINTS, XA_WM_SIZE_HINTS, &size);
    }

    fn open(opts: &Options) -> Clock {
        // SAFETY: Xlib calls on the display opened here; every pointer is
        // checked before it is used.
        unsafe {
            let dpy = XOpenDisplay(ptr::null());
            if dpy.is_null() {
                die("cannot open the display");
            }
            let screen = XDefaultScreen(dpy);
            let root = XRootWindow(dpy, screen);
            let visual = XDefaultVisual(dpy, screen);
            let cmap = XDefaultColormap(dpy, screen);

            let screen_size = (XDisplayWidth(dpy, screen), XDisplayHeight(dpy, screen));
            let (x, y, width) = first_placement(opts, saved_placement(), gui_factor(), screen_size);
            let (width, height) = window_size(width as u32);

            let win = XCreateSimpleWindow(dpy, root, x, y, width as c_uint, height as c_uint, 0, 0, 0);
            set_bytes(dpy, win, XA_WM_NAME, XA_STRING, b"ledclock");
            set_bytes(dpy, win, XA_WM_CLASS, XA_STRING, b"ledclock\0Ledclock\0");
            // A dock window: Window Maker gives it no title bar, no border
            // and no application icon, keeps it out of the window list and
            // puts it on every workspace. Below, it is under the windows;
            // otherwise at the level of the Dock, over them.
            let dock = atom(dpy, "_NET_WM_WINDOW_TYPE_DOCK");
            set_longs(dpy, win, atom(dpy, "_NET_WM_WINDOW_TYPE"), XA_ATOM, &[dock as c_long]);
            let level = if opts.above { "_NET_WM_STATE_ABOVE" } else { "_NET_WM_STATE_BELOW" };
            let state = ["_NET_WM_STATE_STICKY", "_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER", level]
                .map(|name| atom(dpy, name) as c_long);
            set_longs(dpy, win, atom(dpy, "_NET_WM_STATE"), XA_ATOM, &state);
            set_longs(dpy, win, atom(dpy, "_NET_WM_DESKTOP"), XA_CARDINAL, &[0xffff_ffff]);
            // No decorations (flags = MWM_HINTS_DECORATIONS, decorations = 0).
            let motif = atom(dpy, "_MOTIF_WM_HINTS");
            set_longs(dpy, win, motif, motif, &[2, 0, 0, 0, 0]);
            // Never focused (flags = InputHint, input = False).
            set_longs(dpy, win, XA_WM_HINTS, XA_WM_HINTS, &[1, 0, 0, 0, 0, 0, 0, 0, 0]);
            // The pointer moves the window and changes its size: the window
            // manager does neither for a dock window. Only a pointer that
            // has hold of the clock is followed; no Expose is asked for, as
            // the window's background is the picture.
            XSelectInput(dpy, win, BUTTON_PRESS_MASK | BUTTON_RELEASE_MASK | BUTTON1_MOTION_MASK);
            let corners = [XC_TOP_LEFT_CORNER, XC_TOP_RIGHT_CORNER, XC_BOTTOM_LEFT_CORNER, XC_BOTTOM_RIGHT_CORNER]
                .map(|shape| XCreateFontCursor(dpy, shape));
            // A press on a corner goes on to the window under it, which
            // asks for it; `fit` puts the corners in their places.
            let squares = corners.map(|cursor| {
                let square = XCreateWindow(dpy, win, 0, 0, 1, 1, 0, 0, INPUT_ONLY, ptr::null_mut(), 0, ptr::null_mut());
                XDefineCursor(dpy, square, cursor);
                square
            });
            XMapSubwindows(dpy, win);

            let gc = XCreateGC(dpy, win, 0, ptr::null_mut());
            // No event for every copy of a pixmap that had nothing hidden.
            XSetGraphicsExposures(dpy, gc, 0);
            // On the window until `fit` makes the pixmap it draws on.
            let draw = XftDrawCreate(dpy, win, visual, cmap);
            if draw.is_null() {
                die("cannot draw text on this screen");
            }
            let component = |shift: u32| (((opts.color >> shift) & 0xff) * 0x101) as u16;
            let rc = XRenderColor { red: component(16), green: component(8), blue: component(0), alpha: 0xffff };
            let mut fg: XftColor = mem::zeroed();
            if XftColorAllocValue(dpy, visual, cmap, &rc, &mut fg) == 0 {
                die("cannot allocate the colour");
            }

            let mut clock = Clock {
                dpy,
                screen,
                visual,
                depth: XDefaultDepth(dpy, screen),
                win,
                gc,
                draw,
                fg,
                led: [16, 8, 0].map(|shift| ((opts.color >> shift) & 0xff) as f64),
                family: opts.font.clone(),
                sample: Clock::measure(dpy, screen, &opts.font, "M"),
                hours24: opts.hours24,
                fleur: XCreateFontCursor(dpy, XC_FLEUR),
                corners,
                squares,
                x,
                y,
                width,
                height,
                rough: false,
                colors: Vec::new(),
                back: 0,
                pixmap: 0,
                drawn: None,
                digits_x: 0,
                marker: ptr::null_mut(),
                marker_width: 0,
                bearing: 0,
                baseline: 0,
                upper: 0,
            };
            clock.fit(width, height, true);
            clock.set_size_hints();
            clock
        }
    }

    fn screen_size(&self) -> (i32, i32) {
        // SAFETY: valid display and screen.
        unsafe { (XDisplayWidth(self.dpy, self.screen), XDisplayHeight(self.dpy, self.screen)) }
    }

    /// Puts the window at `x`, `y`, `width` pixels wide; `held`, the
    /// pointer still has hold of it and this is not its last size.
    fn place(&mut self, (x, y, width): (i32, i32, i32), held: bool) {
        if (x, y, width) == (self.x, self.y, self.width) {
            if !held {
                self.settle();
            }
            return;
        }
        let (width, height) = window_size(width as u32);
        let resized = width != self.width;
        if resized {
            self.fit(width, height, !held);
        } else if !held {
            self.settle();
        }
        (self.x, self.y) = (x, y);
        self.set_size_hints();
        // SAFETY: valid display, window and pixmap.
        unsafe {
            // The picture of the new size is the background before the
            // window has that size: the X server puts it there at once.
            if resized {
                XSetWindowBackgroundPixmap(self.dpy, self.win, self.pixmap);
            }
            XMoveResizeWindow(self.dpy, self.win, x, y, width as c_uint, height as c_uint);
        }
    }

    /// Smooths the picture of a size the pointer has let go of.
    fn settle(&mut self) {
        if self.rough {
            self.put_face(true);
            self.show((0, 0, self.width, self.height));
        }
    }

    /// The pointer takes hold of the clock by `grip`, or lets go of it
    /// (None). While it has hold of it, the cursor over the whole window
    /// is the one of the grip; then the cursor of the desktop again, and
    /// over the corners their own.
    fn hold(&self, grip: Option<Grip>) {
        // SAFETY: valid display, windows and cursors.
        unsafe {
            match grip {
                Some(grip) => {
                    let cursor = match grip {
                        Grip::Corner { left, top } => self.corners[corner_cursor(left, top)],
                        Grip::Body => self.fleur,
                    };
                    XDefineCursor(self.dpy, self.win, cursor);
                    XUnmapSubwindows(self.dpy, self.win);
                }
                None => {
                    XUndefineCursor(self.dpy, self.win);
                    XMapSubwindows(self.dpy, self.win);
                }
            }
        }
    }

    /// Puts the display of segments, showing `digits`, on the panel:
    /// `left` is where it starts, standing on the baseline. The segments
    /// that are lit have the colour of the LEDs; the others have a little
    /// of it, over the panel. Only the columns that the digits marked in
    /// `which` can touch are drawn; they are the part of the window that
    /// changed, if any.
    fn put_digits(&self, left: i32, digits: [u8; 4], which: [bool; 4]) -> Option<Area> {
        let first = which.iter().position(|&w| w)?;
        let last = which.iter().rposition(|&w| w)?;
        let scale = self.height as f64 / face::HEIGHT;
        let top = self.baseline as f64 - segments::HEIGHT * scale;
        // The pixels the digits can touch: they lean, and an edge of them
        // can be in the middle of a pixel.
        let (x0, x1) = (segments::span(first).0, segments::span(last).1);
        let (x0, x1) = (left + (x0 * scale).floor() as i32 - 1, left + (x1 * scale).ceil() as i32 + 1);
        let (y0, y1) = (top.floor() as i32 - 1, self.baseline + 1);
        let (x0, x1) = (x0.clamp(0, self.width) as usize, x1.clamp(0, self.width) as usize);
        let (y0, y1) = (y0.clamp(0, self.height) as usize, y1.clamp(0, self.height) as usize);
        if x0 == x1 || y0 == y1 {
            return None;
        }
        let mut colors = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for py in y0..y1 {
            for px in x0..x1 {
                let (x, y) = ((px as f64 + 0.5 - left as f64) / scale, (py as f64 + 0.5 - top) / scale);
                let (lit, unlit) = segments::cover(x, y, 1.0 / scale, digits);
                let led = lit + (1.0 - lit) * UNLIT * unlit;
                let under = self.colors[py * self.width as usize + px];
                colors.push([0, 1, 2].map(|k| (under[k] as f64 + (self.led[k] - under[k] as f64) * led).round() as u8));
            }
        }
        self.put_image(self.pixmap, &colors, x0, y0, x1 - x0, y1 - y0);
        Some((x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32))
    }

    /// Draws letters that end at `right`, standing on the line `y`.
    fn letters(&self, s: &str, right: i32, y: i32) {
        let x = right - Clock::advance(self.dpy, self.marker, s);
        // SAFETY: valid draw, colour and font; `s` is UTF-8 of the given
        // length.
        unsafe { XftDrawStringUtf8(self.draw, &self.fg, self.marker, x, y, s.as_ptr(), s.len() as c_int) };
    }

    /// Shows `text` on the window: only what changed is drawn again.
    fn paint(&mut self, text: Text) {
        if let Some(area) = self.draw(text) {
            self.show(area);
        }
    }

    /// Draws `text` on the picture of the window, and gives the part of it
    /// that changed: nothing when the picture shows it already, the digits
    /// that changed when the rest is the same, and else all of it.
    fn draw(&mut self, text: Text) -> Option<Area> {
        let area = match self.drawn {
            Some(drawn) if drawn == text => None,
            Some((marker, digits, charge)) if (marker, charge) == (text.0, text.2) => {
                self.put_digits(self.digits_x, text.1, [0, 1, 2, 3].map(|i| digits[i] != text.1[i]))
            }
            _ => {
                self.draw_all(text);
                Some((0, 0, self.width, self.height))
            }
        };
        self.drawn = Some(text);
        area
    }

    /// Draws the time over the frame and the panel, and on the left of it
    /// the charge of the battery, over AM or PM.
    fn draw_all(&mut self, (marker, digits, charge): Text) {
        let (w, h) = (self.width as c_uint, self.height as c_uint);
        // SAFETY: valid display, pixmaps and GC.
        unsafe { XCopyArea(self.dpy, self.back, self.pixmap, self.gc, 0, 0, w, h, 0, 0) };
        let charge = charge.map(|percent| format!("{percent}%"));
        // The column is as wide as the longer of its two lines, and the
        // margin on its left is measured from where the eye sees them
        // begin.
        let charge_width = charge.as_ref().map_or(0, |s| Clock::advance(self.dpy, self.marker, s));
        let column = self.marker_width.max(charge_width);
        let seen = if charge.is_some() { inset(column, self.marker_width.min(charge_width), self.bearing) } else { 0.0 };
        let (start, digits_x) = text_origins(self.width, column, seen);
        if let Some(marker) = marker {
            self.letters(marker, start + column, self.baseline);
        }
        if let Some(charge) = &charge {
            let line = if marker.is_some() { self.upper } else { self.baseline };
            self.letters(charge, start + column, line);
        }
        self.digits_x = digits_x;
        self.put_digits(digits_x, digits, [true; 4]);
    }

    /// Shows a part of the picture on the window.
    fn show(&self, (x, y, w, h): Area) {
        // SAFETY: valid display, window and pixmap; the area is not empty.
        unsafe {
            // The picture is given to the window again each time: what is
            // drawn into a pixmap after it was made a window's background
            // may not reach the window.
            XSetWindowBackgroundPixmap(self.dpy, self.win, self.pixmap);
            XClearArea(self.dpy, self.win, x, y, w as c_uint, h as c_uint, 0);
            XFlush(self.dpy);
        }
    }
}

/// The hour and the minute of the local time `now`.
fn local_time(now: c_long) -> (i32, i32) {
    // SAFETY: an all-zero `struct tm` is valid, and localtime_r fills it.
    unsafe {
        let mut tm: Tm = mem::zeroed();
        tzset();
        if localtime_r(&now, &mut tm).is_null() {
            return (0, 0);
        }
        (tm.fields[2], tm.fields[1])
    }
}

fn main() {
    let opts = parse_args();
    let mut clock = Clock::open(&opts);
    // The timer is set to a time of the wall clock, so it also fires when
    // the machine wakes from suspend after that time, and it is cancelled,
    // which wakes poll() too, when the clock is set.
    // SAFETY: plain system call.
    let timer = unsafe { timerfd_create(CLOCK_REALTIME, TFD_CLOEXEC) };
    if timer < 0 {
        die("cannot create a timer");
    }
    let mut battery = Battery::default();
    // What the clock showed the last time round.
    let mut shown: Option<Text> = None;
    let mut drag: Option<Drag> = None;
    let mut ev = XEvent::zeroed();
    loop {
        // SAFETY: time() accepts a null pointer.
        let now = unsafe { time(ptr::null_mut()) };
        let (hour, minute) = local_time(now);
        let (marker, digits) = clock_text(hour, minute, opts.hours24);
        // The battery is read again when the minute changes, not for every
        // move of the pointer that has hold of the clock.
        let charge = match shown {
            Some((m, d, charge)) if (m, d) == (marker, digits) => charge,
            _ => battery.charge(),
        };
        let text = (marker, digits, charge);
        clock.paint(text);
        if shown.is_none() {
            // Once its picture is its background: it appears whole.
            // SAFETY: valid display and window.
            unsafe { XMapWindow(clock.dpy, clock.win) };
        }
        shown = Some(text);

        let next = Itimerspec { value: Timespec { sec: now - now.rem_euclid(60) + 60, nsec: 0 }, ..Default::default() };
        let mut fds = [
            PollFd { fd: unsafe { XConnectionNumber(clock.dpy) }, events: POLLIN, revents: 0 },
            PollFd { fd: timer, events: POLLIN, revents: 0 },
        ];
        // SAFETY: XPending also reads what poll() saw; XNextEvent only runs
        // when an event is queued; the timer is read only when it is
        // readable, so read() does not block.
        unsafe {
            if timerfd_settime(timer, TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET, &next, ptr::null_mut()) != 0 {
                die("cannot set the timer");
            }
            if XPending(clock.dpy) == 0 {
                poll(fds.as_mut_ptr(), fds.len() as c_ulong, -1);
            }
            // Where the pointer has dragged the window to. Of the motions
            // that are waiting only the last one counts: the window is not
            // drawn at every size on the way.
            let mut dragged = None;
            while XPending(clock.dpy) > 0 {
                XNextEvent(clock.dpy, &mut ev);
                match ev.kind() {
                    // On the window, or on a corner, from which it comes on
                    // to the window: `x` and `y` are in the window.
                    BUTTON_PRESS if ev.pointer().button == 1 && drag.is_none() => {
                        let p = ev.pointer();
                        let grip = grip_at(clock.width, clock.height, p.x, p.y);
                        clock.hold(Some(grip));
                        drag = Some(Drag {
                            grip,
                            pointer: (p.x_root, p.y_root),
                            x: p.x_root - p.x,
                            y: p.y_root - p.y,
                            width: clock.width,
                            height: clock.height,
                        });
                    }
                    MOTION_NOTIFY => {
                        if let Some(drag) = &drag {
                            let p = ev.pointer();
                            dragged = Some(drag.placement((p.x_root, p.y_root), clock.screen_size()));
                        }
                    }
                    BUTTON_RELEASE if ev.pointer().button == 1 => {
                        if let Some(drag) = drag.take() {
                            let p = ev.pointer();
                            let placement = drag.placement((p.x_root, p.y_root), clock.screen_size());
                            clock.place(placement, false);
                            clock.hold(None);
                            dragged = None;
                            // A click that moved nothing leaves the saved
                            // placement as it is.
                            if placement != (drag.x, drag.y, drag.width) {
                                save_placement(placement);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(placement) = dragged {
                clock.place(placement, true);
            }
            XFlush(clock.dpy);
            if fds[1].revents != 0 {
                let mut expirations = 0u64;
                read(timer, &mut expirations as *mut u64 as *mut _, mem::size_of::<u64>());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions() {
        assert_eq!(parse_position("+10+20"), Some((Offset::Start(10), Offset::Start(20))));
        assert_eq!(parse_position("-0+5"), Some((Offset::End(0), Offset::Start(5))));
        assert_eq!(parse_position("=+7-30"), Some((Offset::Start(7), Offset::End(30))));
        for bad in ["", "+10", "10+20", "+10+", "+a+1", "+1+2+3", "100x50+1+2", "+-1+2"] {
            assert_eq!(parse_position(bad), None, "{bad}");
        }
        assert_eq!(Offset::Start(40).resolve(1000, 300), 40);
        assert_eq!(Offset::End(40).resolve(1000, 300), 660);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("ff0000"), Some(0xff0000));
        assert_eq!(parse_color("#00C0ff"), Some(0x00c0ff));
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("+f0000"), None);
        assert_eq!(parse_color("ff00000"), None);
    }

    #[test]
    fn twelve_hours() {
        assert_eq!(clock_text(7, 15, false), (Some("AM"), [0, 7, 1, 5]));
        assert_eq!(clock_text(0, 5, false), (Some("AM"), [1, 2, 0, 5]));
        assert_eq!(clock_text(12, 0, false), (Some("PM"), [1, 2, 0, 0]));
        assert_eq!(clock_text(23, 59, false), (Some("PM"), [1, 1, 5, 9]));
        assert_eq!(clock_text(0, 5, true), (None, [0, 0, 0, 5]));
        assert_eq!(clock_text(19, 15, true), (None, [1, 9, 1, 5]));
    }

    #[test]
    fn sizes() {
        assert_eq!(window_size(302), (302, 90));
        assert_eq!(window_size(604), (604, 180));
        assert_eq!(window_size(453), (453, 135));
    }

    #[test]
    fn text_in_the_middle() {
        // The panel is 15..286, and with the shadow on its left it looks
        // like 18..286: the middle of that is 152.
        // The display of the digits is 118 wide.
        let (letters, digits) = text_origins(302, 56, 0.0);
        assert_eq!(digits - letters, 56 + 12);
        assert_eq!((letters, digits + 118), (59, 245));
        // The margins: 41 on both sides.
        assert_eq!((letters - 18, 286 - (digits + 118)), (41, 41));
        // Letters that are seen to begin 6 pixels inside their column are
        // 3 further to the left: 44 from where they begin, as on the right.
        let (letters, digits) = text_origins(302, 56, 6.0);
        assert_eq!((letters + 6 - 18, 286 - (digits + 118)), (44, 44));
        // Without letters the digits are in the middle by themselves.
        let (letters, digits) = text_origins(604, 0, 0.0);
        assert_eq!(letters, digits);
        assert_eq!(digits, 304 - 118);
    }

    #[test]
    fn insets() {
        // One line: only the empty part of its first letter.
        assert_eq!(inset(45, 0, 2), 2.0);
        // Two lines of the same length.
        assert_eq!(inset(30, 30, 2), 2.0);
        // AM is 15 shorter than the charge over it: 0.3 of that.
        assert_eq!(inset(45, 30, 2), 6.5);
    }

    #[test]
    fn batteries() {
        assert_eq!(battery_charge(Some("Battery"), Some("System"), Some("75")), Some(75));
        assert_eq!(battery_charge(Some("Battery"), None, Some("100")), Some(100));
        assert_eq!(battery_charge(Some("Battery"), None, Some("103")), Some(100));
        // The adapter, a mouse, and a battery that does not say.
        assert_eq!(battery_charge(Some("Mains"), None, None), None);
        assert_eq!(battery_charge(Some("Battery"), Some("Device"), Some("40")), None);
        assert_eq!(battery_charge(Some("Battery"), Some("System"), None), None);
        assert_eq!(battery_charge(Some("Battery"), Some("System"), Some("full")), None);
        assert_eq!(battery_charge(None, None, Some("75")), None);
    }

    #[test]
    fn factors() {
        assert_eq!(parse_factor(" 1.5"), Some(1.5));
        assert_eq!(parse_factor("20"), Some(MAX_FACTOR));
        assert_eq!(parse_factor("x"), None);
        assert_eq!(parse_factor("0"), None);
    }

    #[test]
    fn grips() {
        // At twice the design a corner reaches 40 pixels in.
        assert_eq!(grip_at(604, 180, 0, 0), Grip::Corner { left: true, top: true });
        assert_eq!(grip_at(604, 180, 39, 39), Grip::Corner { left: true, top: true });
        assert_eq!(grip_at(604, 180, 40, 39), Grip::Body);
        assert_eq!(grip_at(604, 180, 39, 40), Grip::Body);
        assert_eq!(grip_at(604, 180, 603, 0), Grip::Corner { left: false, top: true });
        assert_eq!(grip_at(604, 180, 10, 179), Grip::Corner { left: true, top: false });
        assert_eq!(grip_at(604, 180, 564, 140), Grip::Corner { left: false, top: false });
        assert_eq!(grip_at(604, 180, 302, 90), Grip::Body);
        // The smallest clock still has a middle to move it by.
        assert_eq!(grip_at(60, 18, 30, 9), Grip::Body);
        assert_eq!(grip_at(60, 18, 3, 3), Grip::Corner { left: true, top: true });
        let places = [(true, true), (true, false), (false, true), (false, false)].map(|(l, t)| corner_cursor(l, t));
        assert_eq!({ let mut p = places; p.sort(); p }, [0, 1, 2, 3]);
        assert_eq!(CORNERS.map(|(l, t)| corner_cursor(l, t)), [0, 1, 2, 3]);
    }

    #[test]
    fn corner_squares() {
        // The windows over the corners are where a press takes hold of a
        // corner, and only there.
        for (width, height) in [(604, 180), (198, 59), (60, 18), (8192, 2441)] {
            for (left, top) in CORNERS {
                let (x, y, side) = corner_square(width, height, left, top);
                let corner = Grip::Corner { left, top };
                for (px, py) in [(x, y), (x + side - 1, y), (x, y + side - 1), (x + side - 1, y + side - 1)] {
                    assert_eq!(grip_at(width, height, px, py), corner, "{width} {px},{py}");
                }
                let outside = if left { x + side } else { x - 1 };
                assert_eq!(grip_at(width, height, outside, y), Grip::Body, "{width}");
                let outside = if top { y + side } else { y - 1 };
                assert_eq!(grip_at(width, height, x, outside), Grip::Body, "{width}");
            }
        }
        assert_eq!(corner_square(604, 180, false, false), (564, 140, 40));
    }

    #[test]
    fn moved() {
        let drag = Drag { grip: Grip::Body, pointer: (500, 300), x: 400, y: 250, width: 548, height: 180 };
        let screen = (3024, 1964);
        assert_eq!(drag.placement((500, 300), screen), (400, 250, 548));
        assert_eq!(drag.placement((530, 280), screen), (430, 230, 548));
        // Not off the screen.
        assert_eq!(drag.placement((0, 0), screen), (0, 0, 548));
        assert_eq!(drag.placement((5000, 5000), screen), (3024 - 548, 1964 - 180, 548));
        // A window larger than the screen starts at its edge.
        assert_eq!(on_screen(-5, 300, 200), 0);
        assert_eq!(on_screen(50, 300, 200), 0);
    }

    #[test]
    fn resized() {
        let screen = (3024, 1964);
        let corner = |left, top| Drag {
            grip: Grip::Corner { left, top },
            pointer: (1000, 1000),
            x: 400,
            y: 250,
            width: 604,
            height: 180,
        };
        // The corner at the bottom right: the window stays where it is.
        assert_eq!(corner(false, false).placement((1000, 1000), screen), (400, 250, 604));
        assert_eq!(corner(false, false).placement((1302, 1000), screen), (400, 250, 906));
        // 90 pixels taller is 302 wider.
        assert_eq!(corner(false, false).placement((1000, 1090), screen), (400, 250, 906));
        // Of the two, the one that changes the size more.
        assert_eq!(corner(false, false).placement((1100, 1090), screen), (400, 250, 906));
        assert_eq!(corner(false, false).placement((900, 990), screen), (400, 250, 504));
        assert_eq!(corner(false, false).placement((1000, 955), screen), (400, 250, 453));
        // The corner at the top left: the bottom right one stays at 1004,430.
        let (x, y, width) = corner(true, true).placement((698, 1000), screen);
        assert_eq!((x, y, width), (98, 160, 906));
        assert_eq!((x + width, y + window_size(width as u32).1), (1004, 430));
        let (x, y, width) = corner(true, false).placement((1100, 1000), screen);
        assert_eq!((x, y, width), (500, 250, 504));
        // Within the sizes -width takes.
        assert_eq!(corner(false, false).placement((0, 0), screen).2, MIN_WIDTH as i32);
        assert_eq!(corner(false, false).placement((20000, 1000), screen).2, MAX_WIDTH as i32);
        let (x, y, width) = corner(true, true).placement((3000, 3000), screen);
        assert_eq!((x + width, y + window_size(width as u32).1), (1004, 430));
    }

    #[test]
    fn placements() {
        assert_eq!(parse_placement("1238 90 548\n"), Some((1238, 90, 548)));
        assert_eq!(parse_placement("-3 0 60"), Some((-3, 0, 60)));
        for bad in ["", "1 2", "1 2 3 4", "1 2 59", "1 2 9000", "a 2 300", "1 2 300.5"] {
            assert_eq!(parse_placement(bad), None, "{bad}");
        }

        let screen = (3024, 1964);
        let options = |position, width| Options {
            hours24: false,
            above: false,
            position,
            width,
            color: DEFAULT_COLOR,
            font: DEFAULT_FONT.to_string(),
        };
        // Nothing given, nothing saved.
        assert_eq!(first_placement(&options(None, None), None, 2.0, screen), (1210, 90, 604));
        // What the pointer gave it the last time.
        assert_eq!(first_placement(&options(None, None), Some((100, 200, 411)), 2.0, screen), (100, 200, 411));
        // The options before it, each for what it names.
        let corner = Some((Offset::End(40), Offset::Start(120)));
        assert_eq!(first_placement(&options(corner, None), Some((100, 200, 411)), 2.0, screen), (2573, 120, 411));
        assert_eq!(first_placement(&options(None, Some(300)), Some((100, 200, 411)), 2.0, screen), (100, 200, 300));
        // Saved on a larger screen.
        assert_eq!(first_placement(&options(None, None), Some((2900, 1900, 548)), 2.0, screen), (2476, 1801, 548));
    }
}
