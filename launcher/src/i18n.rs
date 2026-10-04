//! Translations: English is built in (lang/en.lang); other languages are `<code>.lang`
//! files in the game's "scripts/lang" folder, in the same "key = text" format.
//! Texts missing from a translation fall back to English. The translations aren't shipped
//! with the launcher: the Language menu downloads them from `launcher/lang/` in the repo.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

const ENGLISH: &str = include_str!("../lang/en.lang");
const NAME_KEY: &str = "Language";
const LANG_API: &str =
    "https://api.github.com/repos/Pelorojo/NFSUServerChanger/contents/launcher/lang";

type Error = Box<dyn std::error::Error + Send + Sync>;

type Texts = HashMap<String, String>;

static CURRENT: RwLock<Option<Texts>> = RwLock::new(None);

fn english() -> &'static Texts {
    static EN: OnceLock<Texts> = OnceLock::new();
    EN.get_or_init(|| parse(ENGLISH))
}

/// "key = text" lines; ';' starts a comment line, "\n" in a text is a line break.
fn parse(text: &str) -> Texts {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(';') && !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().replace("\\n", "\n")))
        .collect()
}

/// The text for `key` in the current language (English if it's missing there).
pub fn t(key: &str) -> String {
    let current = CURRENT.read().unwrap_or_else(|e| e.into_inner());
    current
        .as_ref()
        .and_then(|texts| texts.get(key))
        .or_else(|| english().get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// `t(key)` with {0}, {1}, ... replaced by `args`.
pub fn tf(key: &str, args: &[&str]) -> String {
    args.iter().enumerate().fold(t(key), |text, (i, arg)| {
        text.replace(&format!("{{{i}}}"), arg)
    })
}

/// A selectable language: file name without ".lang" and its own name.
pub struct Language {
    pub code: String,
    pub name: String,
}

/// English (built in) and every `<code>.lang` in `dir`, sorted by name after English.
pub fn available(dir: Option<&Path>) -> Vec<Language> {
    let mut others: Vec<Language> = dir
        .and_then(|d| fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let code = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
            let is_lang = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lang"));
            if !is_lang || code == "en" {
                return None;
            }
            let texts = parse(&fs::read_to_string(&path).ok()?);
            let name = texts.get(NAME_KEY).cloned().unwrap_or_else(|| code.clone());
            Some(Language { code, name })
        })
        .collect();
    others.sort_by_key(|l| l.name.to_lowercase());
    let mut all = vec![Language {
        code: "en".into(),
        name: english()[NAME_KEY].clone(),
    }];
    all.extend(others);
    all
}

/// Switches to `code` ("en" = built-in English). Returns false if there's no such file.
pub fn set(dir: Option<&Path>, code: &str) -> bool {
    let texts = if code.eq_ignore_ascii_case("en") {
        None
    } else {
        let Some(path) = dir.and_then(|d| find_file(d, code)) else {
            return false;
        };
        match fs::read_to_string(&path) {
            Ok(text) => Some(parse(&text)),
            Err(_) => return false,
        }
    };
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = texts;
    true
}

fn find_file(dir: &Path, code: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_stem()
                .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(code))
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lang"))
        })
}

/// "de.lang" -> "de". Only plain codes, since the name comes from the internet and becomes
/// a file name.
fn lang_code(file_name: &str) -> Option<String> {
    let (stem, ext) = file_name.rsplit_once('.')?;
    let code = stem.to_ascii_lowercase();
    let valid = ext.eq_ignore_ascii_case("lang")
        && !code.is_empty()
        && code.len() <= 16
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then_some(code)
}

/// A translation available for download.
pub struct OnlineLanguage {
    pub code: String,
    pub name: String,
    pub text: String,
}

impl OnlineLanguage {
    /// One of its texts, if it has it.
    pub fn text_for(&self, key: &str) -> Option<String> {
        parse(&self.text).remove(key)
    }
}

/// The translation files in the repo's `launcher/lang/`: (code, download URL), except the
/// built-in English.
fn online_files() -> Result<Vec<(String, String)>, Error> {
    let body = ureq::get(LANG_API).call()?.body_mut().read_to_string()?;
    let entries: serde_json::Value = serde_json::from_str(&body)?;
    Ok(entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let code = lang_code(e["name"].as_str()?).filter(|c| c != "en")?;
            Some((code, e["download_url"].as_str()?.to_string()))
        })
        .collect())
}

fn download(code: &str, url: &str) -> Option<OnlineLanguage> {
    let text = ureq::get(url)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    // Not a language file without its name.
    let name = parse(&text).remove(NAME_KEY)?;
    Some(OnlineLanguage {
        code: code.to_string(),
        name,
        text,
    })
}

/// The translation for `code` from the repo, if there is one (only that file is downloaded).
pub fn online_language(code: &str) -> Result<Option<OnlineLanguage>, Error> {
    Ok(online_files()?
        .into_iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .and_then(|(c, url)| download(&c, &url)))
}

/// All translations in the repo's `launcher/lang/` (except the built-in English),
/// downloaded in parallel and sorted by name.
pub fn online_languages() -> Result<Vec<OnlineLanguage>, Error> {
    let files = online_files()?;
    let mut langs: Vec<OnlineLanguage> = std::thread::scope(|scope| {
        let downloads: Vec<_> = files
            .iter()
            .map(|(code, url)| scope.spawn(move || download(code, url)))
            .collect();
        downloads
            .into_iter()
            .filter_map(|d| d.join().ok().flatten())
            .collect()
    });
    langs.sort_by_key(|l| l.name.to_lowercase());
    Ok(langs)
}

/// The installed file's text for `code`, if there is one.
pub fn installed_text(dir: &Path, code: &str) -> Option<String> {
    fs::read_to_string(find_file(dir, code)?).ok()
}

/// Writes a downloaded translation into `dir` (replacing an installed one).
pub fn install(dir: &Path, lang: &OnlineLanguage) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path =
        find_file(dir, &lang.code).unwrap_or_else(|| dir.join(format!("{}.lang", lang.code)));
    fs::write(path, &lang.text)
}

pub fn remove(dir: &Path, code: &str) -> std::io::Result<()> {
    match find_file(dir, code) {
        Some(path) => fs::remove_file(path),
        None => Ok(()),
    }
}

/// The system's language as a code like "de" (from "de-DE"), English if unknown.
pub fn system_language() -> String {
    sys_locale::get_locale()
        .and_then(|l| l.split(['-', '_']).next().map(str::to_ascii_lowercase))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "en".into())
}

#[cfg(test)]
#[path = "tests/i18n.rs"]
mod tests;
