//! Unit tests for `ping.rs` (a child module of it, so private items are reachable).

use super::*;

const STATUS: &str = "Name:\tNFSUServerChanger\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nGroups:\t4 27 1000 \n";

#[test]
fn default_range_allows_nobody() {
    assert_eq!(allows("1\t0\n", STATUS), Some(false));
}

#[test]
fn full_range_allows_everyone() {
    assert_eq!(allows("0\t2147483647\n", STATUS), Some(true));
}

#[test]
fn supplementary_group_is_enough() {
    // Effective gid 1000 is outside, group 27 (sudo) inside.
    assert_eq!(allows("20 30", STATUS), Some(true));
    assert_eq!(allows("28 999", STATUS), Some(false));
}

#[test]
fn no_supplementary_groups() {
    let status = "Gid:\t100\t100\t100\t100\nGroups:\t\n";
    assert_eq!(allows("100 100", status), Some(true));
    assert_eq!(allows("101 200", status), Some(false));
}

#[test]
fn unreadable_input() {
    assert_eq!(allows("", STATUS), None);
    assert_eq!(allows("0 x", STATUS), None);
    assert_eq!(allows("0 10", "Name:\tx\n"), None);
}

#[test]
fn wine_versions() {
    assert_eq!(supports("wine-6.0.3"), Some(false));
    assert_eq!(supports("wine-7.0 (Staging)"), Some(false));
    assert_eq!(supports("wine-7.12"), Some(false));
    assert_eq!(supports("wine-7.13"), Some(true));
    assert_eq!(supports("wine-8.0-rc1"), Some(true));
    assert_eq!(supports("wine-10.0\n"), Some(true));
    assert_eq!(supports("wine-9"), Some(true));
    assert_eq!(supports("something else"), None);
    assert_eq!(supports("wine-x"), None);
}
