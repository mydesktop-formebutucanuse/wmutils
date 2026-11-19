// SPDX-License-Identifier: GPL-3.0-or-later
//! What the backlights were set to, kept for the next start (after a
//! restart of the daemon or of the machine) in
//! `$XDG_STATE_HOME/autobrightd/state`. One line per backlight:
//!
//! ```text
//! screen 2159 64000 manual 12.5
//! kbd 0 255 auto
//! ```
//!
//! name, where it was heading, the max_brightness that value is for, and
//! whether the user had set it, with the smoothed lux at that moment.

use crate::logic::Kept;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

/// What one backlight was set to.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// The max_brightness the target is for: the kernel patches change the
    /// units.
    pub max: u32,
    pub kept: Kept,
}

/// Where autobrightd keeps its files: $XDG_STATE_HOME/autobrightd.
pub fn dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(base.join("autobrightd"))
}

/// The entries in `text`. Lines that do not parse are left out: a file from
/// another version must not stop the daemon.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Entry> {
    let mut words = line.split_whitespace();
    let name = words.next().filter(|n| !n.starts_with('#'))?.to_owned();
    let target = words.next()?.parse().ok()?;
    let max = words.next()?.parse().ok()?;
    let override_lux = match (words.next()?, words.next()) {
        ("auto", None) => None,
        ("manual", Some(lux)) => Some(lux.parse::<f32>().ok().filter(|l| l.is_finite())?),
        _ => return None,
    };
    Some(Entry { name, max, kept: Kept { target, override_lux } })
}

pub fn format(entries: &[Entry]) -> String {
    let mut text = String::from(
        "# autobrightd: what the backlights were set to, for the next start\n\
         # name, value, max_brightness, auto | manual and the lux at that moment\n",
    );
    for e in entries {
        let mode = match e.kept.override_lux {
            Some(lux) => format!("manual {lux}"),
            None => "auto".into(),
        };
        text.push_str(&format!("{} {} {} {mode}\n", e.name, e.kept.target, e.max));
    }
    text
}

/// The saved entries; none if nothing was saved yet.
pub fn load() -> Result<Vec<Entry>, String> {
    let Some(path) = dir().map(|d| d.join("state")) else { return Ok(Vec::new()) };
    match fs::read_to_string(&path) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Replaces the saved entries. The file is written beside the old one and
/// renamed over it, so a crash leaves one of the two, never half a file.
pub fn save(entries: &[Entry]) -> Result<(), String> {
    let dir = dir().ok_or("no HOME")?;
    let (path, tmp) = (dir.join("state"), dir.join("state.new"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(&tmp, format(entries)).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_read_back() {
        let entries = vec![
            Entry { name: "screen".into(), max: 64000, kept: Kept { target: 2159, override_lux: Some(12.537) } },
            Entry { name: "kbd".into(), max: 255, kept: Kept { target: 0, override_lux: None } },
        ];
        assert_eq!(parse(&format(&entries)), entries);
    }

    #[test]
    fn lines_that_do_not_parse_are_left_out() {
        let text = "# comment\n\nscreen 100\nscreen x 500 auto\nscreen 100 500 manual\n\
                    screen 100 500 manual NaN\nscreen 100 500 auto 3\nkbd 7 255 auto\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], Entry { name: "kbd".into(), max: 255, kept: Kept { target: 7, override_lux: None } });
    }
}
