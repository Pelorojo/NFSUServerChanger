//! Unit tests for `lutris.rs` (a child module of it, so private items are reachable).

use super::*;

#[test]
fn reads_a_lutris_entry() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-lutris-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let game_dir = dir.join("prefix/drive_c/Games/ug1");
    fs::create_dir_all(&game_dir).unwrap();
    let yml = dir.join("kickstart-nfsu-netplay-launcher-1738845962.yml");
    // Long paths get folded over two lines like Lutris writes them.
    fs::write(
        &yml,
        format!(
            "game:\n  exe: {0}/prefix/drive_c/Games/ug1/kickStart.exe\n  prefix: {0}/prefix\n  working_dir: {0}/prefix/drive_c/Games/ug1\nsystem:\n  env:\n    DXVK_HUD: fps\nwine:\n  overrides:\n    dinput8.dll: native,builtin\n  version: some-wine-that-isnt-there\n",
            dir.display()
        ),
    )
    .unwrap();

    let game = game_from(&yml, &load_yaml(&yml).unwrap());
    assert_eq!(game.name, "kickstart-nfsu-netplay-launcher");
    assert_eq!(game.prefix, Some(dir.join("prefix")));
    assert_eq!(game.wine_version, "some-wine-that-isnt-there");
    assert_eq!(game.wine, None); // not installed: falls back to the system's wine
    assert_eq!(
        game.overrides,
        [("dinput8.dll".to_string(), "native,builtin".to_string())]
    );
    assert_eq!(game.env, [("DXVK_HUD".to_string(), "fps".to_string())]);
    fs::remove_dir_all(&dir).unwrap();
}
