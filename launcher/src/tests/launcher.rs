//! Unit tests for `launcher.rs` (a child module of it, so private items are reachable).

use super::*;

#[test]
fn finds_speed_executables() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-games-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for name in [
        "Speed_widescreen.EXE",
        "SPEED.exe",
        "speed.ini",
        "other.exe",
    ] {
        fs::write(dir.join(name), "").unwrap();
    }
    let names: Vec<String> = find_games(&dir)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["SPEED.exe", "Speed_widescreen.EXE"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn expands_command() {
    let exe = Path::new("/games/nfsu/speed.exe");
    let q = quote(exe);
    assert_eq!(
        expand_command("wine {exe} -window", "{exe}", exe),
        format!("wine {q} -window")
    );
    assert_eq!(
        expand_command("gamemoderun wine ", "{exe}", exe),
        format!("gamemoderun wine {q}")
    );
    assert_eq!(
        expand_command("code -n {file}", "{file}", exe),
        format!("code -n {q}")
    );
}

#[test]
fn desktop_entries() {
    let entry = "[Desktop Entry]\nName=Text Editor\nExec=gnome-text-editor --new-window %U\nTerminal=false\n[Desktop Action new]\nExec=other\n";
    assert_eq!(
        desktop_value(entry, "Exec").as_deref(),
        Some("gnome-text-editor --new-window %U")
    );
    assert_eq!(desktop_value(entry, "Terminal").as_deref(), Some("false"));
    let file = Path::new("/games/servers.dat");
    let q = quote(file);
    assert_eq!(
        desktop_exec("gnome-text-editor --new-window %U", file),
        format!("gnome-text-editor --new-window {q}")
    );
    assert_eq!(desktop_exec("kate -b %i", file), format!("kate -b {q}"));
}

#[test]
fn detects_wine_prefix() {
    let exe = Path::new("/home/u/Games/nfsu/drive_c/Games/ug1/speed.exe");
    assert_eq!(wine_prefix(exe), Some(Path::new("/home/u/Games/nfsu")));
    assert_eq!(wine_prefix(Path::new("/mnt/e/Games/nfsu/speed.exe")), None);
}

#[test]
fn dll_overrides_for_game_folder() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-dlls-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(dll_overrides(&dir, &[]), None);
    for name in ["dinput8.dll", "D3D9.DLL", "speed.exe", "readme.txt"] {
        fs::write(dir.join(name), "").unwrap();
    }
    assert_eq!(
        dll_overrides(&dir, &[]).as_deref(),
        Some("d3d9=n,b;dinput8=n,b")
    );
    // Lutris overrides are added and win for the same DLL.
    let lutris = [
        ("D3D9.dll".to_string(), "builtin".to_string()),
        ("dxgi.dll".to_string(), "native,builtin".to_string()),
    ];
    assert_eq!(
        dll_overrides(&dir, &lutris).as_deref(),
        Some("d3d9=b;dinput8=n,b;dxgi=n,b")
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn override_modes() {
    assert_eq!(short_override("native,builtin"), "n,b");
    assert_eq!(short_override("builtin"), "b");
    assert_eq!(short_override("disabled"), "");
}

#[test]
fn writable_dirs() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-writable-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    assert!(dir_writable(&dir));
    // The test file is removed again.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    assert!(!dir_writable(&dir.join("missing")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn matches_running_games() {
    assert!(process_matches("Speed1Euro.exe", "speed1euro.exe"));
    assert!(!process_matches("Speed1.exe", "Speed1Euro.exe"));
    // Linux cuts process names at 15 characters.
    assert!(process_matches("Speed_Widescree", "Speed_Widescreen.exe"));
    assert!(!process_matches("Speed", "Speed1Euro.exe"));
}
