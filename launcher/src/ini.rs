//! Minimal INI access. Only the one key being written is touched; comments,
//! other keys/sections and line endings stay exactly as they were.

use std::fs;
use std::path::Path;

const BOM: char = '\u{feff}';

/// The file as text, without a UTF-8 BOM (some editors add one; it would hide the
/// first section header). Also tells whether there was one, to keep it on writing.
fn read_text(path: &Path) -> std::io::Result<(String, bool)> {
    let text = String::from_utf8_lossy(&fs::read(path)?).into_owned();
    Ok(match text.strip_prefix(BOM) {
        Some(rest) => (rest.to_string(), true),
        None => (text, false),
    })
}

fn is_section(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if line.trim_start().starts_with(';') {
        return None;
    }
    let (k, v) = line.split_once('=')?;
    k.trim().eq_ignore_ascii_case(key).then(|| v.trim())
}

pub fn read_value(path: &Path, section: &str, key: &str) -> Option<String> {
    let (text, _) = read_text(path).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        if let Some(name) = is_section(line) {
            in_section = name.eq_ignore_ascii_case(section);
        } else if in_section {
            if let Some(v) = key_value(line, key) {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn write_value(path: &Path, section: &str, key: &str, value: &str) -> std::io::Result<()> {
    let (text, bom) = read_text(path)?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let new_line = format!("{key} = {value}");

    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut in_section = false;
    let mut header = None;
    let mut replaced = false;
    for (i, line) in lines.iter_mut().enumerate() {
        if let Some(name) = is_section(line) {
            in_section = name.eq_ignore_ascii_case(section);
            if in_section {
                header = Some(i);
            }
        } else if in_section && key_value(line, key).is_some() {
            *line = new_line.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        match header {
            Some(i) => lines.insert(i + 1, new_line),
            None => {
                lines.insert(0, String::new());
                lines.insert(0, new_line);
                lines.insert(0, format!("[{section}]"));
            }
        }
    }

    let mut out = if bom { BOM.to_string() } else { String::new() };
    out.push_str(&lines.join(eol));
    if text.ends_with('\n') {
        out.push_str(eol);
    }
    // Write to a temp file first so a failed write never leaves a half-written ini.
    let tmp = path.with_extension("ini.tmp");
    fs::write(&tmp, out)?;
    fs::rename(&tmp, path)
}

/// All `key = value` entries of a section in file order, or None if the section doesn't exist.
pub fn read_section(path: &Path, section: &str) -> Option<Vec<(String, String)>> {
    let (text, _) = read_text(path).ok()?;
    let mut entries = None;
    let mut in_section = false;
    for line in text.lines() {
        if let Some(name) = is_section(line) {
            in_section = name.eq_ignore_ascii_case(section);
            if in_section {
                entries.get_or_insert_with(Vec::new);
            }
        } else if in_section && !line.trim_start().starts_with(';') {
            if let Some((k, v)) = line.split_once('=') {
                entries
                    .get_or_insert_with(Vec::new)
                    .push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }
    entries
}

/// The text split into sections: (name, block with the [name] line and everything up to
/// the next section). Text before the first section has the name "".
pub fn sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![(String::new(), String::new())];
    for line in text.lines() {
        if let Some(name) = is_section(line) {
            out.push((name.to_string(), String::new()));
        }
        let block = &mut out.last_mut().unwrap().1;
        block.push_str(line);
        block.push('\n');
    }
    out
}

/// Adds what `defaults` has and `existing` lacks, leaving everything present untouched:
/// missing sections are appended, missing keys are inserted at the end of their section
/// together with the comment lines directly above them in `defaults`. Sections named in
/// `whole_only` (curated lists) are only added when missing entirely.
pub fn merge_defaults(existing: &str, defaults: &str, whole_only: &[&str]) -> String {
    let (existing, bom) = match existing.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (existing, false),
    };
    let defaults = defaults.strip_prefix(BOM).unwrap_or(defaults);
    let eol = if existing.contains("\r\n") || existing.is_empty() && defaults.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();

    for (name, block) in sections(defaults) {
        if name.is_empty() {
            continue;
        }
        let Some(header) = lines
            .iter()
            .position(|l| is_section(l).is_some_and(|n| n.eq_ignore_ascii_case(&name)))
        else {
            // Whole section missing: append it.
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.extend(block.trim_end().lines().map(str::to_string));
            continue;
        };
        if whole_only.iter().any(|w| w.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let end = lines[header + 1..]
            .iter()
            .position(|l| is_section(l).is_some())
            .map_or(lines.len(), |i| header + 1 + i);
        let has_key = |key: &str| {
            lines[header + 1..end]
                .iter()
                .any(|l| key_value(l, key).is_some())
        };

        let mut to_add: Vec<String> = Vec::new();
        let mut comments: Vec<String> = Vec::new();
        for line in block.lines().skip(1) {
            let trimmed = line.trim_start();
            if trimmed.starts_with(';') {
                comments.push(line.to_string());
            } else if let Some((key, _)) = line.split_once('=') {
                if !has_key(key.trim()) {
                    to_add.append(&mut comments);
                    to_add.push(line.to_string());
                }
                comments.clear();
            } else {
                comments.clear();
            }
        }
        if !to_add.is_empty() {
            // After the section's last non-empty line.
            let mut at = end;
            while at > header + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.splice(at..at, to_add);
        }
    }

    let mut out = if bom { BOM.to_string() } else { String::new() };
    out.push_str(&lines.join(eol));
    out.push_str(eol);
    out
}

#[cfg(test)]
#[path = "tests/ini.rs"]
mod tests;
