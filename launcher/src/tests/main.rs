//! Unit tests for `main.rs` (a child module of it, so private items are reachable).

use super::*;

#[test]
fn custom_list_roundtrip() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-list-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(CUSTOM_LIST_NAME);
    fs::write(
        &path,
        "31.131.19.86\r\n\r\n 91.240.86.90 \r\n31.131.19.86\r\n",
    )
    .unwrap();
    let mut list = read_custom_list(&path);
    assert_eq!(list, ["31.131.19.86", "91.240.86.90"]);
    list.push("nfs.onl".into());
    write_custom_list(&path, &list).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "31.131.19.86\r\n91.240.86.90\r\nnfs.onl\r\n"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rejects_bad_hosts() {
    assert!(validate_host("racepark.nfs.onl").is_ok());
    assert!(validate_host("").is_ok());
    assert!(validate_host("localhost").is_ok());
    assert!(validate_host("51.254.141.170").is_ok());
    assert!(validate_host("3dnet.example").is_ok());
    assert!(validate_host("bad host").is_err());
    assert!(validate_host("1.2.3.999").is_err());
    assert!(validate_host("1.2.3").is_err());
    assert!(validate_host("nfs..onl").is_err());
    assert!(validate_host(".nfs.onl").is_err());
    assert!(validate_host("nfs.onl.").is_err());
    assert!(validate_host("-nfs.onl").is_err());
    assert!(validate_host("nfs-.onl").is_err());
    assert!(validate_host("nfsu.online:10901").is_ok());
    assert!(validate_host("51.254.141.170:5000").is_ok());
    assert!(validate_host("nfsu.online:0").is_err());
    assert!(validate_host("nfsu.online:abc").is_err());
    assert!(validate_host("nfsu.online:").is_err());
    assert!(validate_host(&format!("{}:10901", "a".repeat(31))).is_ok());
    assert!(validate_host(&"a".repeat(31)).is_ok());
    assert!(validate_host(&"a".repeat(32)).is_err());
}
