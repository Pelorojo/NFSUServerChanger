//! Linux: whether the game's pings work, and switching them on.
//!
//! The game pings servers over a raw ICMP socket. Since 7.13, Wine can emulate that with the
//! kernel's unprivileged ping sockets, which `net.ipv4.ping_group_range` allows per group
//! (default on many distros: "1 0" = nobody). Without them the plugin keeps the game from
//! crashing, but the pings time out.

use std::fs;
use std::path::Path;
use std::process::Command;

const CONF_FILE: &str = "/etc/sysctl.d/99-nfsu-ping.conf";
/// Allows ping sockets for every group, now and after reboots. Run as root.
const SCRIPT: &str = "echo 'net.ipv4.ping_group_range = 0 2147483647' > /etc/sysctl.d/99-nfsu-ping.conf && sysctl -p /etc/sysctl.d/99-nfsu-ping.conf";
/// First Wine with the ping socket emulation.
const MIN_WINE: (u32, u32) = (7, 13);

pub enum State {
    /// Pings work (or it can't be told: no false alarms).
    On,
    /// Not allowed for this user's groups.
    Off,
    /// Not allowed, and the game's Wine (this version) couldn't use them anyway.
    OldWine(String),
}

/// `wine`: the Wine the game is started with; None = unknown (custom start command).
pub fn state(wine: Option<&Path>) -> State {
    if allowed() {
        return State::On;
    }
    match wine.and_then(wine_version) {
        Some(version) if !supports(&version).unwrap_or(true) => State::OldWine(version),
        _ => State::Off,
    }
}

/// The command shown to users who'd rather run it themselves.
pub fn command() -> String {
    format!("sudo sh -c \"{SCRIPT}\"")
}

/// Runs the command as root via polkit (the system's own password dialog). Blocks until the
/// dialog is closed.
pub fn enable() -> Result<(), String> {
    let status = Command::new("pkexec")
        .args(["sh", "-c", SCRIPT])
        .status()
        .map_err(|e| format!("pkexec: {e}"))?;
    match status.code() {
        Some(0) if allowed() => Ok(()),
        Some(0) => Err(format!("{CONF_FILE}: no effect")),
        // pkexec: 126 = dialog dismissed, 127 = not authorized / no polkit agent.
        Some(126) => Err(crate::i18n::t("ping.cancelled")),
        _ => Err(format!("pkexec: {status}")),
    }
}

/// False only when the range is readable and none of this process's groups is in it;
/// anything unreadable counts as allowed (no false alarms).
fn allowed() -> bool {
    let (Ok(range), Ok(status)) = (
        fs::read_to_string("/proc/sys/net/ipv4/ping_group_range"),
        fs::read_to_string("/proc/self/status"),
    ) else {
        return true;
    };
    allows(&range, &status).unwrap_or(true)
}

/// Whether a group from `status` (/proc/self/status: effective gid, supplementary groups)
/// lies in `range` ("low high", inclusive). None if either can't be parsed.
fn allows(range: &str, status: &str) -> Option<bool> {
    let mut bounds = range.split_whitespace().map(str::parse::<u32>);
    let (low, high) = (bounds.next()?.ok()?, bounds.next()?.ok()?);

    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(|rest| rest.split_whitespace().map(str::parse::<u32>))
    };
    // "Gid:" is real, effective, saved, filesystem; the kernel checks the effective one.
    let egid = field("Gid:")?.nth(1)?.ok()?;
    let groups = field("Groups:").into_iter().flatten().flatten();

    Some(
        std::iter::once(egid)
            .chain(groups)
            .any(|g| (low..=high).contains(&g)),
    )
}

/// `wine --version` output, e.g. "wine-9.0 (Staging)".
fn wine_version(wine: &Path) -> Option<String> {
    let output = Command::new(wine).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Whether a `wine --version` output is at least MIN_WINE. None if it can't be parsed.
fn supports(version: &str) -> Option<bool> {
    let number = version.trim().strip_prefix("wine-")?;
    let mut parts = number
        .split(|c: char| !c.is_ascii_digit())
        .map(str::parse::<u32>);
    let major = parts.next()?.ok()?;
    // "wine-8.0-rc1", "wine-9.0": a missing minor counts as 0.
    let minor = parts.next().and_then(Result::ok).unwrap_or(0);
    Some((major, minor) >= MIN_WINE)
}

#[cfg(test)]
#[path = "tests/ping.rs"]
mod tests;
