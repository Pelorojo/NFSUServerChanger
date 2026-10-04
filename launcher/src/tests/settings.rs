//! Unit tests for `settings.rs` (a child module of it, so private items are reachable).

use super::*;
use std::fs;

#[test]
fn list_sections() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-settings-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("NFSUServerChanger.ini");
    fs::write(
        &path,
        "[PublicList]\r\n; comment\r\nA = a.example\r\nC = https://c.example/list?x=1\r\n[Community]\r\nB & C = https://b/\r\n",
    )
    .unwrap();
    assert_eq!(
        providers(Some(&path)),
        ["a.example", "https://c.example/list?x=1"]
    );
    assert_eq!(
        community_links(Some(&path)),
        [("B & C".to_string(), "https://b/".to_string())]
    );
    assert!(providers(None).is_empty());

    // Only the public lists named in [Settings] are queried.
    save(&path, "notepad++ {file}", &["C".to_string()]).unwrap();
    assert_eq!(providers(Some(&path)), ["https://c.example/list?x=1"]);
    assert_eq!(editor(Some(&path)), "notepad++ {file}");
    assert_eq!(
        public_lists(Some(&path))[0],
        ("A".to_string(), "a.example".to_string(), false)
    );
    fs::remove_dir_all(&dir).unwrap();
}
