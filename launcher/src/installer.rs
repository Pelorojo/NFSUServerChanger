//! Installs / updates the NFSUServerChanger plugin and the launcher itself from the
//! GitHub releases.
//!
//! The plugin zip holds `dinput8.dll` (ASI loader), `readme.txt` and `scripts/`
//! with the .asi, .ini and trax .csv; it is extracted into the game folder, except
//! readme.txt and the trax .csv.
//!
//! Launcher releases are separate assets in the same repo,
//! `NFSU.Server.Changer.Launcher.v<version>.Windows.zip` / `.Linux.tar.gz`, holding the binary.
//! The running binary is renamed to `.old` and the new one put in its place (Windows allows
//! renaming a running .exe, just not overwriting it); the `.old` is deleted at the next start.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::ini;

// Sections that are lists the user curates: added only when missing entirely, so
// deliberately removed servers/links don't come back with an update.
const LIST_SECTIONS: &[&str] = &["FixedServers", "PublicList", "Community", "Help"];

const RELEASES_API: &str =
    "https://api.github.com/repos/Pelorojo/NFSUServerChanger/releases?per_page=30";

/// Plugin release assets are "NFSU.Server.Changer.v<version>.zip"; launcher releases in the
/// same repo ("NFSU.Server.Changer.Launcher...") don't match, so they're skipped.
fn is_plugin_asset(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("nfsu.server.changer.v") && name.ends_with(".zip")
}

const LAUNCHER_ASSET_PREFIX: &str = "nfsu.server.changer.launcher.v";
#[cfg(windows)]
const LAUNCHER_ASSET_SUFFIX: &str = ".windows.zip";
#[cfg(not(windows))]
const LAUNCHER_ASSET_SUFFIX: &str = ".linux.tar.gz";
#[cfg(windows)]
const LAUNCHER_BINARY: &str = "NFSUServerChanger.exe";
#[cfg(not(windows))]
const LAUNCHER_BINARY: &str = "NFSUServerChanger";

/// The version in a launcher asset name for this platform
/// ("NFSU.Server.Changer.Launcher.v1.0.0.1001.Windows.zip" -> 1.0.0.1001).
fn launcher_asset_version(name: &str, suffix: &str) -> Option<Version> {
    let name = name.to_ascii_lowercase();
    parse_version(
        name.strip_prefix(LAUNCHER_ASSET_PREFIX)?
            .strip_suffix(suffix)?,
    )
}

type Error = Box<dyn std::error::Error + Send + Sync>;

const MAX_DOWNLOAD: u64 = 100 * 1024 * 1024;

pub struct Release {
    pub version: String,
    url: String,
    /// "sha256:<hex>"; GitHub only has it for assets uploaded since mid-2025.
    digest: Option<String>,
}

/// A version like 1.0.0.1993, from a "v1.0.0.1993" tag or the .asi's version info.
pub type Version = [u32; 4];

pub fn parse_version(text: &str) -> Option<Version> {
    let mut parts = text
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    let mut version = [0; 4];
    for slot in &mut version {
        *slot = parts.next().flatten().unwrap_or(0);
    }
    (version != [0; 4]).then_some(version)
}

/// The file version from a DLL's version resource (VS_FIXEDFILEINFO, found by its signature).
pub fn file_version(path: &Path) -> Option<Version> {
    let data = fs::read(path).ok()?;
    let at = data
        .windows(4)
        .position(|w| w == [0xBD, 0x04, 0xEF, 0xFE])?;
    let word = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            data.get(at + offset..at + offset + 4)?.try_into().ok()?,
        ))
    };
    let (ms, ls) = (word(8)?, word(12)?);
    Some([ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF])
}

impl Release {
    /// Whether this release is newer than the installed plugin (or there's none / no version info).
    pub fn is_newer_than(&self, installed: Option<&Path>) -> bool {
        match (
            parse_version(&self.version),
            installed.and_then(file_version),
        ) {
            (Some(latest), Some(current)) => latest > current,
            (_, None) => true,
            (None, Some(_)) => false,
        }
    }
}

/// The newest plugin and launcher release, from one look at the releases.
pub struct Updates {
    pub plugin: Option<Release>,
    pub launcher: Option<Release>,
}

/// Published (non-draft, non-prerelease) releases, newest first.
fn published_releases() -> Result<Vec<serde_json::Value>, Error> {
    let body = ureq::get(RELEASES_API)
        .call()?
        .body_mut()
        .read_to_string()?;
    let releases: serde_json::Value = serde_json::from_str(&body)?;
    Ok(releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true && r["prerelease"] != true)
        .cloned()
        .collect())
}

fn plugin_release(releases: &[serde_json::Value]) -> Option<Release> {
    releases.iter().find_map(|r| {
        let asset = r["assets"]
            .as_array()?
            .iter()
            .find(|a| a["name"].as_str().is_some_and(is_plugin_asset))?;
        Some(Release {
            version: r["tag_name"].as_str().unwrap_or_default().to_string(),
            url: asset["browser_download_url"].as_str()?.to_string(),
            digest: asset["digest"].as_str().map(String::from),
        })
    })
}

/// The highest launcher version for this platform (by asset name, whatever the tag is).
fn launcher_release(releases: &[serde_json::Value], suffix: &str) -> Option<Release> {
    releases
        .iter()
        .filter_map(|r| r["assets"].as_array())
        .flatten()
        .filter_map(|a| {
            let version = launcher_asset_version(a["name"].as_str()?, suffix)?;
            Some((version, a))
        })
        .max_by_key(|(version, _)| *version)
        .and_then(|(version, asset)| {
            Some(Release {
                version: version.map(|p| p.to_string()).join("."),
                url: asset["browser_download_url"].as_str()?.to_string(),
                digest: asset["digest"].as_str().map(String::from),
            })
        })
}

pub fn check() -> Result<Updates, Error> {
    let releases = published_releases()?;
    Ok(Updates {
        plugin: plugin_release(&releases),
        launcher: launcher_release(&releases, LAUNCHER_ASSET_SUFFIX),
    })
}

/// The newest release that has a plugin zip.
pub fn latest_release() -> Result<Release, Error> {
    check()?
        .plugin
        .ok_or_else(|| crate::i18n::t("msg.no_plugin_release").into())
}

/// Downloads an asset, checking its SHA-256 when GitHub has one.
fn download(release: &Release) -> Result<Vec<u8>, Error> {
    // ureq stops at 10 MB by default; the launcher archives are about that size already.
    let data = ureq::get(&release.url)
        .call()?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()?;
    if let Some(expected) = release
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
    {
        let actual: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(crate::i18n::t("msg.download_corrupted").into());
        }
    }
    Ok(data)
}

/// Downloads the release and extracts it into `game_dir`. Returns what was skipped.
pub fn install(release: &Release, game_dir: &Path) -> Result<Vec<String>, Error> {
    extract(&download(release)?, game_dir)
}

// --- Launcher self-update ---------------------------------------------------

/// This launcher's version, e.g. 1.0.0.1000 (Cargo keeps the build number as "+1000").
pub fn launcher_version() -> Version {
    parse_version(&env!("CARGO_PKG_VERSION").replace('+', ".")).unwrap_or_default()
}

impl Release {
    pub fn is_newer_launcher(&self) -> bool {
        parse_version(&self.version).is_some_and(|v| v > launcher_version())
    }
}

/// The running launcher's path, taken once at start: on Linux, current_exe() would name the
/// renamed ".old" file after an update.
pub fn launcher_path() -> Option<&'static Path> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| std::env::current_exe().ok()).as_deref()
}

fn sibling(path: &Path, extension: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(extension);
    path.with_file_name(name)
}

/// Deletes what an update left behind. Right after "Restart launcher" the old process may
/// still be exiting (Windows can't delete a running .exe), so it's retried for a few seconds.
pub fn clean_up_launcher_update() {
    let Some(exe) = launcher_path() else { return };
    let _ = fs::remove_file(sibling(exe, ".new"));
    let old = sibling(exe, ".old");
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..40 {
            if fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

/// Downloads a launcher release and puts it in place of the running binary; it's used from
/// the next start on.
pub fn update_launcher(release: &Release) -> Result<(), Error> {
    let data = download(release)?;
    let binary = if cfg!(windows) {
        binary_from_zip(&data, LAUNCHER_BINARY)?
    } else {
        binary_from_tar_gz(&data, LAUNCHER_BINARY)?
    };
    let exe = launcher_path().ok_or("launcher path unknown")?;
    replace_binary(exe, &binary)
}

fn binary_from_zip(data: &[u8], name: &str) -> Result<Vec<u8>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let matches = file
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.eq_ignore_ascii_case(name)))
            .unwrap_or(false);
        if matches && file.is_file() {
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            return Ok(content);
        }
    }
    Err(format!("{name} not found in the download").into())
}

fn binary_from_tar_gz(data: &[u8], name: &str) -> Result<Vec<u8>, Error> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(data));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let matches = entry.path()?.file_name().is_some_and(|n| n == name);
        if matches && entry.header().entry_type().is_file() {
            let mut content = Vec::new();
            entry.read_to_end(&mut content)?;
            return Ok(content);
        }
    }
    Err(format!("{name} not found in the download").into())
}

/// Writes `binary` next to `exe`, then swaps: exe -> exe.old, new -> exe.
fn replace_binary(exe: &Path, binary: &[u8]) -> Result<(), Error> {
    let new = sibling(exe, ".new");
    let old = sibling(exe, ".old");
    fs::write(&new, binary)?;
    // Same permissions as the running one (the executable bit on Linux).
    if let Ok(meta) = fs::metadata(exe) {
        fs::set_permissions(&new, meta.permissions())?;
    }
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(exe, &old) {
        let _ = fs::remove_file(&new);
        return Err(e.into());
    }
    if let Err(e) = fs::rename(&new, exe) {
        let _ = fs::rename(&old, exe);
        return Err(e.into());
    }
    Ok(())
}

/// Starts the (updated) launcher again with the same arguments; the caller then quits.
pub fn restart_launcher() -> std::io::Result<()> {
    let exe = launcher_path().ok_or(std::io::ErrorKind::NotFound)?;
    // Marked as a restart, so it waits for this process to quit instead of handing over to it.
    std::process::Command::new(exe)
        .args(
            std::env::args_os()
                .skip(1)
                .filter(|a| a != crate::single_instance::RESTARTED_ARG),
        )
        .arg(crate::single_instance::RESTARTED_ARG)
        .spawn()
        .map(|_| ())
}

fn extract(zip_data: &[u8], game_dir: &Path) -> Result<Vec<String>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_data))?;
    let mut skipped = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        // enclosed_name: rejects absolute paths and "..", nothing lands outside the game folder.
        let Some(name) = file.enclosed_name() else {
            continue;
        };
        if file.is_dir() {
            continue;
        }
        let file_name = name
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let target = resolve_ci(game_dir, &name);
        // readme.txt isn't needed; the trax list is only an example of the game's own titles
        // (the launcher creates it from its template when it's edited first).
        if file_name == "readme.txt" || file_name == "nfsuserverchangertrax.csv" {
            continue;
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        if target.exists() {
            match file_name.as_str() {
                // Any existing loader does the job (often the same one, e.g. from a widescreen fix).
                "dinput8.dll" => {
                    skipped.push("dinput8.dll (already there)".to_string());
                    continue;
                }
                // Keep the user's settings; only add what the new version brings along.
                "nfsuserverchanger.ini" => {
                    let old = String::from_utf8_lossy(&fs::read(&target)?).into_owned();
                    let merged = ini::merge_defaults(
                        &old,
                        &String::from_utf8_lossy(&content),
                        LIST_SECTIONS,
                    );
                    if merged != old {
                        fs::write(&target, merged)?;
                    }
                    continue;
                }
                _ => {}
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content)?;
    }
    Ok(skipped)
}

/// `relative` under `base`, reusing existing entries that only differ in case
/// (so "scripts" also finds "Scripts" on Linux).
fn resolve_ci(base: &Path, relative: &Path) -> PathBuf {
    let mut path = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        let existing = fs::read_dir(&path).ok().and_then(|entries| {
            entries.flatten().map(|e| e.file_name()).find(|n| {
                n.to_string_lossy()
                    .eq_ignore_ascii_case(&part.to_string_lossy())
            })
        });
        path.push(existing.as_deref().unwrap_or(part));
    }
    path
}

#[cfg(test)]
#[path = "tests/installer.rs"]
mod tests;
