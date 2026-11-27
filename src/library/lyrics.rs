// SPDX-License-Identifier: GPL-3.0-only

//! Synced `.lrc` and plain-text lyrics loader & parser.

use crate::library::metadata::read_embedded_lyrics;
use crate::settings::LyricsPriority;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    /// Timestamp in milliseconds when synced (LRC), or `None` for plain static lyrics.
    pub time_ms: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLyrics {
    pub is_synced: bool,
    pub lines: Vec<LyricLine>,
    pub source_label: String,
}

impl ParsedLyrics {
    /// Returns the index of the currently active synced lyric line for `position_ms`.
    pub fn active_line_index(&self, position_ms: u64) -> Option<usize> {
        if !self.is_synced || self.lines.is_empty() {
            return None;
        }
        let mut active = None;
        for (idx, line) in self.lines.iter().enumerate() {
            if let Some(ts) = line.time_ms {
                if ts <= position_ms {
                    active = Some(idx);
                } else {
                    break;
                }
            }
        }
        active
    }
}

/// Loads lyrics for a track using the user's preferred source priority
/// (sidecar `.lrc` file vs. embedded lyrics tags).
pub fn load_lyrics_for_track(path: &Path, priority: LyricsPriority) -> Option<ParsedLyrics> {
    let load_sidecar = || {
        let lrc_path = path.with_extension("lrc");
        if lrc_path.is_file()
            && let Ok(raw) = fs::read_to_string(&lrc_path)
            && let Some(parsed) = parse_lyrics(&raw, "LRC file")
        {
            return Some(parsed);
        }
        None
    };

    let load_embedded = || {
        if let Some(embedded) = read_embedded_lyrics(path)
            && let Some(parsed) = parse_lyrics(&embedded, "Embedded")
        {
            return Some(parsed);
        }
        None
    };

    match priority {
        LyricsPriority::SidecarFirst => load_sidecar().or_else(load_embedded),
        LyricsPriority::EmbeddedFirst => load_embedded().or_else(load_sidecar),
    }
}

/// Parses either timestamped LRC lyrics (`[mm:ss.xx] Line`) or plain unsynchronized lyrics.
pub fn parse_lyrics(raw: &str, source: &str) -> Option<ParsedLyrics> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut synced_lines: Vec<LyricLine> = Vec::new();
    let mut offset_ms: i64 = 0;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse optional LRC `[offset:+/-ms]` tag
        if let Some(rest) = line.strip_prefix("[offset:")
            && let Some(val_str) = rest.strip_suffix(']')
            && let Ok(ms) = val_str.trim().parse::<i64>()
        {
            offset_ms = ms;
            continue;
        }

        // Extract one or more `[mm:ss.xx]` prefixes on the same line
        let mut rest = line;
        let mut timestamps = Vec::new();
        while rest.starts_with('[') {
            if let Some(close) = rest.find(']') {
                let inside = &rest[1..close];
                if let Some(ms) = parse_lrc_timestamp(inside) {
                    timestamps.push(ms);
                    rest = &rest[close + 1..];
                    continue;
                }
            }
            break;
        }

        if !timestamps.is_empty() {
            let text = rest.trim().to_string();
            for ts in timestamps {
                let adjusted = (ts as i64 - offset_ms).max(0) as u64;
                synced_lines.push(LyricLine {
                    time_ms: Some(adjusted),
                    text: text.clone(),
                });
            }
        }
    }

    if !synced_lines.is_empty() {
        synced_lines.sort_by_key(|l| l.time_ms.unwrap_or(0));
        return Some(ParsedLyrics {
            is_synced: true,
            lines: synced_lines,
            source_label: format!("{source} • Synced"),
        });
    }

    let plain_lines: Vec<LyricLine> = raw
        .lines()
        .map(|l| LyricLine {
            time_ms: None,
            text: l.trim_end().to_string(),
        })
        .collect();

    if plain_lines.iter().all(|l| l.text.trim().is_empty()) {
        return None;
    }

    Some(ParsedLyrics {
        is_synced: false,
        lines: plain_lines,
        source_label: source.to_string(),
    })
}

/// Parses `mm:ss`, `mm:ss.xx`, or `mm:ss.xxx` (also supports `:` as sub-second separator).
fn parse_lrc_timestamp(s: &str) -> Option<u64> {
    let mut parts = s.splitn(2, ':');
    let mins: u64 = parts.next()?.trim().parse().ok()?;
    let sec_part = parts.next()?.trim();

    let (secs, millis) = if let Some((sec_s, frac_s)) = sec_part
        .split_once('.')
        .or_else(|| sec_part.split_once(':'))
    {
        let secs: u64 = sec_s.parse().ok()?;
        let frac_trimmed: String = frac_s.chars().take(3).collect();
        let raw_frac: u64 = frac_trimmed.parse().ok()?;
        let millis = match frac_trimmed.len() {
            1 => raw_frac * 100,
            2 => raw_frac * 10,
            3 => raw_frac,
            _ => 0,
        };
        (secs, millis)
    } else {
        (sec_part.parse().ok()?, 0)
    };

    Some(mins * 60_000 + secs * 1_000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synced_lrc_and_finds_active_line() {
        let lrc = "\
[ti:Test Song]
[00:05.00]First line
[00:12.50][00:30.00]Chorus line
[00:20.250]Second verse
";
        let parsed = parse_lyrics(lrc, "Test").expect("should parse");
        assert!(parsed.is_synced);
        assert_eq!(parsed.lines.len(), 4);
        assert_eq!(parsed.lines[0].text, "First line");
        assert_eq!(parsed.lines[1].text, "Chorus line");
        assert_eq!(parsed.lines[2].text, "Second verse");
        assert_eq!(parsed.lines[3].text, "Chorus line");

        assert_eq!(parsed.active_line_index(4_999), None);
        assert_eq!(parsed.active_line_index(5_000), Some(0));
        assert_eq!(parsed.active_line_index(15_000), Some(1));
        assert_eq!(parsed.active_line_index(25_000), Some(2));
        assert_eq!(parsed.active_line_index(35_000), Some(3));
    }

    #[test]
    fn parses_plain_unsynced_lyrics() {
        let plain = "Line one\nLine two\n\nLine four";
        let parsed = parse_lyrics(plain, "Embedded").expect("should parse");
        assert!(!parsed.is_synced);
        assert_eq!(parsed.lines.len(), 4);
        assert_eq!(parsed.active_line_index(10_000), None);
    }
}
