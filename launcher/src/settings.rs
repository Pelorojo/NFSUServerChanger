//! The launcher's settings live in the plugin's own NFSUServerChanger.ini, in
//! sections the plugin ignores ([FixedServers], [PublicList], [Community], [Help], [Settings],
//! [Launch]). The ini
//! shipped with the plugin has them preset; list sections are `Name = value`
//! lines, used in file order.

use std::path::Path;

use crate::ini;

fn list_section(ini_path: Option<&Path>, section: &str) -> Vec<(String, String)> {
    ini_path
        .and_then(|p| ini::read_section(p, section))
        .unwrap_or_default()
}

/// All public lists from [PublicList] as (name, hostname or URL, enabled).
/// Enabled ones are named in `[Settings] PublicLists`; without that key all are.
pub fn public_lists(ini_path: Option<&Path>) -> Vec<(String, String, bool)> {
    let enabled = ini_path.and_then(|p| ini::read_value(p, "Settings", "PublicLists"));
    let is_enabled = |name: &str| match &enabled {
        Some(list) => list.split(',').any(|n| n.trim().eq_ignore_ascii_case(name)),
        None => true,
    };
    list_section(ini_path, "PublicList")
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, value)| {
            let on = is_enabled(&name);
            (name, value, on)
        })
        .collect()
}

/// The public lists to query: hostname (-> http://<host>/tracker/get_list.php) or full URL.
pub fn providers(ini_path: Option<&Path>) -> Vec<String> {
    public_lists(ini_path)
        .into_iter()
        .filter(|(_, _, on)| *on)
        .map(|(_, value, _)| value)
        .collect()
}

/// Editor command for the Edit menu entries; empty = system default.
pub fn editor(ini_path: Option<&Path>) -> String {
    ini_path
        .and_then(|p| ini::read_value(p, "Settings", "Editor"))
        .unwrap_or_default()
}

pub fn save(ini_path: &Path, editor: &str, enabled_lists: &[String]) -> std::io::Result<()> {
    ini::write_value(ini_path, "Settings", "Editor", editor)?;
    ini::write_value(
        ini_path,
        "Settings",
        "PublicLists",
        &enabled_lists.join(", "),
    )
}

/// Fixed public servers (name, hostname or IP), always queried and shown while online.
pub fn fixed_servers(ini_path: Option<&Path>) -> Vec<(String, String)> {
    list_section(ini_path, "FixedServers")
        .into_iter()
        .filter(|(_, host)| !host.is_empty())
        .collect()
}

/// Help menu links (name, URL).
pub fn help_links(ini_path: Option<&Path>) -> Vec<(String, String)> {
    list_section(ini_path, "Help")
}

/// Community menu links (name, URL).
pub fn community_links(ini_path: Option<&Path>) -> Vec<(String, String)> {
    list_section(ini_path, "Community")
}

pub fn last_game(ini_path: Option<&Path>) -> String {
    ini_path
        .and_then(|p| ini::read_value(p, "Launch", "Game"))
        .unwrap_or_default()
}

pub fn set_last_game(ini_path: Option<&Path>, name: &str) {
    if let Some(path) = ini_path {
        if last_game(ini_path) != name {
            let _ = ini::write_value(path, "Launch", "Game", name);
        }
    }
}

/// The chosen language code (`[Settings] Language`); empty = the system's language.
pub fn language(ini_path: Option<&Path>) -> String {
    ini_path
        .and_then(|p| ini::read_value(p, "Settings", "Language"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

pub fn set_language(ini_path: Option<&Path>, code: &str) {
    if let Some(path) = ini_path {
        if language(ini_path) != code {
            let _ = ini::write_value(path, "Settings", "Language", code);
        }
    }
}

#[cfg(not(windows))]
/// Linux: whether to offer switching pings on at start (`[Settings] PingHint`, 0 = no).
pub fn ping_hint(ini_path: Option<&Path>) -> bool {
    ini_path
        .and_then(|p| ini::read_value(p, "Settings", "PingHint"))
        .is_none_or(|v| v.trim() != "0")
}

pub fn set_ping_hint(ini_path: &Path, ask: bool) -> std::io::Result<()> {
    ini::write_value(
        ini_path,
        "Settings",
        "PingHint",
        if ask { "1" } else { "0" },
    )
}

pub fn set_launch_command(ini_path: &Path, command: &str) -> std::io::Result<()> {
    if launch_command(Some(ini_path)) == command {
        return Ok(());
    }
    ini::write_value(ini_path, "Launch", "Command", command)
}

pub fn launch_command(ini_path: Option<&Path>) -> String {
    ini_path
        .and_then(|p| ini::read_value(p, "Launch", "Command"))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/settings.rs"]
mod tests;
