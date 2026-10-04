//! Unit tests for `ini.rs` (a child module of it, so private items are reachable).

use super::*;

#[test]
fn replaces_only_server_host() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("NFSUServerChanger.ini");
    let original = "[Server]\r\n; Hostname or IP address\r\nHost = nfs.onl\r\n\r\n[EATrax]\r\nNoQuotes = 0\r\n";
    fs::write(&path, original).unwrap();

    assert_eq!(
        read_value(&path, "Server", "Host").as_deref(),
        Some("nfs.onl")
    );
    write_value(&path, "Server", "Host", "nfsu.online").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original.replace("nfs.onl", "nfsu.online")
    );
    write_value(&path, "Server", "Host", "").unwrap();
    assert_eq!(read_value(&path, "Server", "Host").as_deref(), Some(""));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn merges_defaults() {
    let existing = "[Server]\r\nHost = mine\r\n\r\n[FixedServers]\r\nMine = my.host\r\n\r\n[Launch]\r\nCommand = wine\r\n";
    let defaults = "; head\r\n[Server]\r\n; host comment\r\nHost = nfs.onl\r\n\r\n[FixedServers]\r\nA = a.host\r\n\r\n\
                    [Launch]\r\n; cmd\r\nCommand =\r\n; last game\r\nGame =\r\n\r\n[Community]\r\n; links\r\nX = http://x/\r\n";
    let merged = merge_defaults(existing, defaults, &["FixedServers", "Community"]);
    assert_eq!(
        merged,
        "[Server]\r\nHost = mine\r\n\r\n[FixedServers]\r\nMine = my.host\r\n\r\n[Launch]\r\nCommand = wine\r\n\
         ; last game\r\nGame =\r\n\r\n[Community]\r\n; links\r\nX = http://x/\r\n"
    );
    // Nothing missing: unchanged.
    assert_eq!(
        merge_defaults(&merged, defaults, &["FixedServers", "Community"]),
        merged
    );
}

#[test]
fn sections_in_order() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-sect-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.ini");
    fs::write(&path, "[Server]\r\nHost = nfs.onl\r\n[Community]\r\n; c\r\nB & A = https://b/?x=1\r\nA = http://a/\r\n").unwrap();

    assert!(read_section(&path, "Missing").is_none());
    let entries = read_section(&path, "Community").unwrap();
    assert_eq!(
        entries,
        [
            ("B & A".into(), "https://b/?x=1".into()),
            ("A".into(), "http://a/".into())
        ]
    );
    assert_eq!(
        read_value(&path, "Server", "Host").as_deref(),
        Some("nfs.onl")
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keeps_bom() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-bom-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bom.ini");
    fs::write(&path, "\u{feff}[Server]\r\nHost = nfs.onl\r\n").unwrap();

    // The first section is found despite the BOM, and the BOM stays on writing.
    assert_eq!(
        read_value(&path, "Server", "Host").as_deref(),
        Some("nfs.onl")
    );
    write_value(&path, "Server", "Host", "nfsu.online").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "\u{feff}[Server]\r\nHost = nfsu.online\r\n"
    );
    assert_eq!(
        merge_defaults("\u{feff}[Server]\nHost = x\n", "[Server]\nHost = y\n", &[]),
        "\u{feff}[Server]\nHost = x\n"
    );

    fs::remove_dir_all(&dir).unwrap();
}
