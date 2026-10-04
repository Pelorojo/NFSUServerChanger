//! Unit tests for `i18n.rs` (a child module of it, so private items are reachable).

use super::*;

fn repo_lang_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("lang")
}

/// The {0}, {1}, ... in a text.
fn placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> = (0..10)
        .map(|i| format!("{{{i}}}"))
        .filter(|p| text.contains(p.as_str()))
        .collect();
    found.sort();
    found
}

#[test]
fn parse_lines() {
    let texts = parse(
        "\u{feff}; comment\r\nLanguage = Deutsch\r\na.b = x = y\r\nc = one\\ntwo\r\n\r\nbroken\r\n",
    );
    assert_eq!(texts["Language"], "Deutsch");
    assert_eq!(texts["a.b"], "x = y");
    assert_eq!(texts["c"], "one\ntwo");
    assert_eq!(texts.len(), 3);
}

// Switching languages changes global state, so it's all in one test.
#[test]
fn switch_and_fallback() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-i18n-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("XX.Lang"), "Language = Testish\nmain.add = Plus\n").unwrap();
    fs::write(dir.join("notes.txt"), "Language = no\n").unwrap();

    let names: Vec<String> = available(Some(&dir)).into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["English", "Testish"]);

    assert!(set(Some(&dir), "xx"));
    assert_eq!(t("main.add"), "Plus");
    // Missing in the translation: English.
    assert_eq!(t("main.remove"), "Remove");
    assert_eq!(tf("status.online", &["3"]), "3 servers online.");
    assert_eq!(t("no.such.key"), "no.such.key");

    assert!(!set(Some(&dir), "zz"));
    assert_eq!(t("main.add"), "Plus");
    assert!(set(None, "en"));
    assert_eq!(t("main.add"), "Add");
    fs::remove_dir_all(&dir).unwrap();
}

/// Every shipped translation has exactly the English keys with the same placeholders.
#[test]
fn translations_complete() {
    let english = english();
    let langs = available(Some(&repo_lang_dir()));
    assert!(langs.len() > 1);
    for lang in langs.iter().filter(|l| l.code != "en") {
        let path = find_file(&repo_lang_dir(), &lang.code).unwrap();
        let texts = parse(&fs::read_to_string(path).unwrap());
        for (key, text) in english {
            let Some(translated) = texts.get(key) else {
                panic!("{}.lang: {key} missing", lang.code);
            };
            assert_eq!(
                placeholders(text),
                placeholders(translated),
                "{}.lang: {key}",
                lang.code
            );
            // Windows menus take '&' as a mnemonic marker.
            assert!(!translated.contains('&'), "{}.lang: {key}", lang.code);
        }
        for key in texts.keys() {
            assert!(
                english.contains_key(key),
                "{}.lang: unknown {key}",
                lang.code
            );
        }
    }
}

#[test]
fn download_names() {
    assert_eq!(lang_code("de.lang").as_deref(), Some("de"));
    assert_eq!(lang_code("PT-br.LANG").as_deref(), Some("pt-br"));
    assert_eq!(lang_code("readme.md"), None);
    assert_eq!(lang_code(".lang"), None);
    assert_eq!(lang_code("../evil.lang"), None);
    assert_eq!(lang_code("a b.lang"), None);
}

#[test]
fn install_and_remove() {
    let dir = std::env::temp_dir()
        .join(format!("nfsu-sel-i18n-dl-{}", std::process::id()))
        .join("lang");
    let lang = OnlineLanguage {
        code: "xx".into(),
        name: "Testish".into(),
        text: "Language = Testish\n".into(),
    };
    // Creates the missing folder.
    install(&dir, &lang).unwrap();
    assert_eq!(
        installed_text(&dir, "XX").as_deref(),
        Some("Language = Testish\n")
    );
    remove(&dir, "xx").unwrap();
    assert_eq!(installed_text(&dir, "xx"), None);
    remove(&dir, "xx").unwrap();
    fs::remove_dir_all(dir.parent().unwrap()).unwrap();
}
