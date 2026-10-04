//! Unit tests for `servers.rs` (a child module of it, so private items are reachable).

use super::*;

fn entry(host: &str, ip: &str, reachable: bool) -> Entry {
    Entry {
        host: host.into(),
        ip: ip.into(),
        title: if reachable {
            host.to_uppercase()
        } else {
            String::new()
        },
        reachable,
        has_stats: reachable,
        ..Default::default()
    }
}

#[test]
fn custom_servers_always_shown() {
    let cached = vec![
        entry("nfs.onl", "51.83.11.17", true),
        entry("zerolan.ru", "91.240.86.90", true),
        entry("31.131.19.86", "", false),
        entry("5.135.184.181", "", false),
    ];
    let custom = [
        "91.240.86.90".to_string(),
        "31.131.19.86".to_string(),
        "unknown.example".to_string(),
    ];
    let defaults = [(
        "kickRom Motor City".to_string(),
        "motorcity.nfs.onl".to_string(),
    )];
    let known = known_servers(&cached, &defaults, &custom);
    let list = merge_custom(&known, &custom);
    let hosts: Vec<(&str, bool, bool, bool)> = list
        .iter()
        .map(|s| (s.host.as_str(), s.reachable, s.custom, s.checking))
        .collect();
    assert_eq!(
        hosts,
        [
            ("nfs.onl", true, false, false),
            ("91.240.86.90", true, true, false),
            ("31.131.19.86", false, true, false),
            ("unknown.example", false, true, true),
        ]
    );
    assert_eq!(list[1].title, "ZEROLAN.RU");
    // Defaults missing from the cache are added, custom ones stay out of the cache.
    assert!(known
        .iter()
        .any(|e| e.host == "motorcity.nfs.onl" && e.pending && e.title == "kickRom Motor City"));
    assert!(known
        .iter()
        .any(|e| e.host == "unknown.example" && e.custom_only));
}

#[test]
fn host_ports() {
    assert_eq!(
        split_host_port("nfsu.online:10901"),
        ("nfsu.online", Some(10901))
    );
    assert_eq!(split_host_port("nfsu.online"), ("nfsu.online", None));
    assert_eq!(split_host_port("nfsu.online:0"), ("nfsu.online:0", None));
    assert_eq!(
        split_host_port("nfsu.online:65536"),
        ("nfsu.online:65536", None)
    );
    assert_eq!(split_host_port(":10901"), ("", Some(10901)));
    let e = Entry {
        host: "nfsu.online".into(),
        ip: "159.253.22.191".into(),
        ..Default::default()
    };
    assert!(matches(&e, "nfsu.online:10901"));
    assert!(matches(&e, "159.253.22.191:10901"));
    // A bare ":port" matches nothing, not even entries without an IP yet.
    let unresolved = Entry {
        host: "nfs.onl".into(),
        ..Default::default()
    };
    assert!(!matches(&unresolved, ":10901"));
}

#[test]
fn cache_roundtrip() {
    let path = std::env::temp_dir().join(format!("nfsu-sel-cache-{}", std::process::id()));
    let custom = Entry {
        host: "custom.example".into(),
        custom_only: true,
        ..Default::default()
    };
    let gone = entry("1.2.3.4", "1.2.3.4", false);
    let listed_offline = Entry {
        listed: true,
        ..entry("5.6.7.8", "5.6.7.8", false)
    };
    let all = [
        entry("nfs.onl", "51.83.11.17", true),
        custom,
        gone,
        listed_offline,
    ];

    save_cache(&path, &all, true);
    let hosts: Vec<String> = load_cache(&path).into_iter().map(|e| e.host).collect();
    assert_eq!(hosts, ["nfs.onl", "5.6.7.8"]);
    // Public lists unavailable: nothing is pruned.
    save_cache(&path, &all, false);
    assert_eq!(load_cache(&path).len(), 3);
    fs::remove_file(&path).unwrap();
}

#[test]
fn public_list_ips() {
    // Values as returned by the trackers, decoded like servers.php does.
    assert_eq!(decode_ip("386441433").as_deref(), Some("217.160.8.23"));
    assert_eq!(decode_ip("0"), None);
    assert_eq!(decode_ip("junk"), None);

    let mut list = vec![entry("racepark.nfs.onl", "217.160.8.23", true)];
    add_public(
        &mut list,
        Some(&["217.160.8.23".to_string(), "45.7.228.197".to_string()]),
    );
    assert_eq!(list.len(), 2);
    assert!(list[0].listed);
    assert!(list[1].pending && list[1].host == "45.7.228.197");
}
