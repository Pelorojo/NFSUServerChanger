//! Linux: when the game folder belongs to a Lutris game, start the game with that
//! entry's Wine settings (prefix, Wine version, DLL overrides, environment).

use std::fs;
use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

/// What a Lutris game entry says about how to run Wine.
pub struct LutrisGame {
    /// The entry's config name without the install timestamp, e.g. "kickstart-nfsu-netplay-launcher".
    pub name: String,
    pub prefix: Option<PathBuf>,
    /// Wine binary; None = the system's `wine`.
    pub wine: Option<PathBuf>,
    /// Name of the Wine version as Lutris shows it ("system", "wine-ge-8-26-x86_64", ...).
    pub wine_version: String,
    /// DLL overrides ("dinput8" -> "native,builtin").
    pub overrides: Vec<(String, String)>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
}

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
}

fn load_yaml(path: &Path) -> Option<Yaml> {
    YamlLoader::load_from_str(&fs::read_to_string(path).ok()?)
        .ok()?
        .into_iter()
        .next()
}

fn map_pairs(yaml: &Yaml) -> Vec<(String, String)> {
    yaml.as_hash()
        .map(|hash| {
            hash.iter()
                .filter_map(|(k, v)| {
                    let value = match v {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k.as_str()?.to_string(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The Lutris game whose working directory, exe folder or Wine prefix contains `game_dir`.
/// An entry pointing exactly at the folder wins over one that only shares the prefix.
pub fn find_for(game_dir: &Path) -> Option<LutrisGame> {
    let game_dir = game_dir.canonicalize().ok()?;
    let dirs = [
        data_dir().map(|d| d.join("lutris/games")),
        config_dir().map(|d| d.join("lutris/games")),
    ];
    let mut best: Option<(u8, PathBuf, Yaml)> = None;
    for dir in dirs.into_iter().flatten() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "yml") {
                continue;
            }
            let Some(yaml) = load_yaml(&path) else {
                continue;
            };
            let game = &yaml["game"];
            let canonical = |key: &str| {
                game[key]
                    .as_str()
                    .and_then(|p| Path::new(p).canonicalize().ok())
            };
            let exe_dir = canonical("exe").and_then(|p| p.parent().map(Path::to_path_buf));
            // 2 = this very folder, 1 = somewhere in the entry's prefix.
            let score = if canonical("working_dir").as_deref() == Some(&game_dir)
                || exe_dir.as_deref() == Some(&game_dir)
            {
                2
            } else if canonical("prefix").is_some_and(|prefix| game_dir.starts_with(prefix)) {
                1
            } else {
                0
            };
            if score > best.as_ref().map_or(0, |b| b.0) {
                best = Some((score, path, yaml));
            }
        }
    }
    let (_, path, yaml) = best?;
    Some(game_from(&path, &yaml))
}

fn game_from(path: &Path, yaml: &Yaml) -> LutrisGame {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    // "kickstart-nfsu-netplay-launcher-1738845962" -> without the install timestamp.
    let name = match stem.rsplit_once('-') {
        Some((name, stamp)) if !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()) => {
            name.to_string()
        }
        _ => stem.to_string(),
    };

    // The game's Wine version, else the Wine runner's default.
    let runner = [
        data_dir().map(|d| d.join("lutris/runners/wine.yml")),
        config_dir().map(|d| d.join("lutris/runners/wine.yml")),
    ]
    .into_iter()
    .flatten()
    .find_map(|p| load_yaml(&p));
    let wine_version = yaml["wine"]["version"]
        .as_str()
        .or_else(|| runner.as_ref().and_then(|r| r["wine"]["version"].as_str()))
        .unwrap_or("system")
        .to_string();
    let wine = (wine_version != "system")
        .then(|| {
            data_dir().map(|d| {
                d.join("lutris/runners/wine")
                    .join(&wine_version)
                    .join("bin/wine")
            })
        })
        .flatten()
        .filter(|p| p.is_file());

    LutrisGame {
        name,
        prefix: yaml["game"]["prefix"].as_str().map(PathBuf::from),
        wine,
        wine_version,
        overrides: map_pairs(&yaml["wine"]["overrides"]),
        env: map_pairs(&yaml["system"]["env"]),
    }
}

#[cfg(test)]
#[path = "tests/lutris.rs"]
mod tests;
