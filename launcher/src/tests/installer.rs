//! Unit tests for `installer.rs` (a child module of it, so private items are reachable).

use super::*;
use std::io::Write;

#[test]
fn plugin_assets() {
    assert!(is_plugin_asset("NFSU.Server.Changer.v1.0.0.1993.zip"));
    assert!(!is_plugin_asset("NFSU.Server.Changer.Launcher.v0.1.0.zip"));
    assert!(!is_plugin_asset("NFSU.Server.Changer.v1.0.0.1993.7z"));
}

#[test]
fn versions() {
    assert_eq!(parse_version("v1.0.0.1993"), Some([1, 0, 0, 1993]));
    assert_eq!(parse_version("1.2"), Some([1, 2, 0, 0]));
    assert_eq!(parse_version("latest"), None);

    let dir = std::env::temp_dir().join(format!("nfsu-sel-ver-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let asi = dir.join("test.asi");
    // Minimal VS_FIXEDFILEINFO: signature, struct version, FileVersionMS/LS for 1.0.0.1993.
    let mut data = b"MZ padding".to_vec();
    data.extend([0xBD, 0x04, 0xEF, 0xFE, 0, 0, 1, 0]);
    data.extend((1u32 << 16).to_le_bytes()); // MS: 1.0
    data.extend(1993u32.to_le_bytes()); // LS: 0.1993
    fs::write(&asi, data).unwrap();
    assert_eq!(file_version(&asi), Some([1, 0, 0, 1993]));

    let release = |v: &str| Release {
        version: v.into(),
        url: String::new(),
        digest: None,
    };
    assert!(release("v1.0.0.1994").is_newer_than(Some(&asi)));
    assert!(!release("v1.0.0.1993").is_newer_than(Some(&asi)));
    assert!(release("v1.0.0.1993").is_newer_than(None));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn extracts_like_expected() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("Scripts")).unwrap();
    fs::write(dir.join("dinput8.dll"), "old loader").unwrap();
    fs::write(dir.join("Scripts/NFSUServerChangerTrax.csv"), "my titles").unwrap();
    fs::write(
        dir.join("Scripts/NFSUServerChanger.ini"),
        "[Server]\r\nHost = mine\r\n",
    )
    .unwrap();

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, content) in [
            ("dinput8.dll", "new loader"),
            ("readme.txt", "readme"),
            ("scripts/NFSUServerChanger.asi", "asi"),
            (
                "scripts/NFSUServerChanger.ini",
                "[Server]\r\nHost = nfs.onl\r\n\r\n[Community]\r\nA = http://a/\r\n",
            ),
            ("scripts/NFSUServerChangerTrax.csv", "shipped titles"),
            ("../evil.txt", "nope"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    let skipped = extract(buf.get_ref(), &dir).unwrap();
    assert_eq!(skipped, ["dinput8.dll (already there)"]);
    // The user's track list isn't replaced by the shipped one.
    assert_eq!(
        fs::read_to_string(dir.join("Scripts/NFSUServerChangerTrax.csv")).unwrap(),
        "my titles"
    );
    assert_eq!(
        fs::read_to_string(dir.join("dinput8.dll")).unwrap(),
        "old loader"
    );
    assert!(!dir.join("readme.txt").exists());
    assert!(!dir.parent().unwrap().join("evil.txt").exists());
    // Existing "Scripts" folder reused, asi written, ini kept + missing section added.
    assert_eq!(
        fs::read_to_string(dir.join("Scripts/NFSUServerChanger.asi")).unwrap(),
        "asi"
    );
    assert!(!dir.join("scripts").exists() || dir.join("scripts") == dir.join("Scripts"));
    assert_eq!(
        fs::read_to_string(dir.join("Scripts/NFSUServerChanger.ini")).unwrap(),
        "[Server]\r\nHost = mine\r\n\r\n[Community]\r\nA = http://a/\r\n"
    );
    fs::remove_dir_all(&dir).unwrap();

    // A fresh install gets neither readme.txt nor the .csv (trax renamer stays off).
    fs::create_dir_all(&dir).unwrap();
    assert!(extract(buf.get_ref(), &dir).unwrap().is_empty());
    assert!(!dir.join("scripts/NFSUServerChangerTrax.csv").exists());
    assert!(dir.join("scripts/NFSUServerChanger.asi").exists());
    assert!(dir.join("dinput8.dll").exists() && !dir.join("readme.txt").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn launcher_assets() {
    let v = |name: &str| launcher_asset_version(name, ".windows.zip");
    assert_eq!(
        v("NFSU.Server.Changer.Launcher.v1.0.0.1001.Windows.zip"),
        Some([1, 0, 0, 1001])
    );
    assert_eq!(
        v("NFSU.Server.Changer.Launcher.v1.0.0.1001.Linux.tar.gz"),
        None
    );
    assert_eq!(v("NFSU.Server.Changer.v1.0.0.1994.zip"), None);
    assert!(!is_plugin_asset(
        "NFSU.Server.Changer.Launcher.v1.0.0.1001.Windows.zip"
    ));

    // The highest version for the platform, wherever it is in the list.
    let asset = |name: &str| serde_json::json!({"name": name, "browser_download_url": format!("https://x/{name}")});
    let releases = vec![
        serde_json::json!({"tag_name": "v1.0.0.1995", "assets": [asset("NFSU.Server.Changer.v1.0.0.1995.zip")]}),
        serde_json::json!({"tag_name": "launcher-a", "assets": [
            asset("NFSU.Server.Changer.Launcher.v1.0.0.1002.Linux.tar.gz"),
        ]}),
        serde_json::json!({"tag_name": "launcher-b", "assets": [
            asset("NFSU.Server.Changer.Launcher.v1.0.0.1001.Windows.zip"),
            asset("NFSU.Server.Changer.Launcher.v1.0.0.1001.Linux.tar.gz"),
        ]}),
    ];
    let win = launcher_release(&releases, ".windows.zip").unwrap();
    assert_eq!(win.version, "1.0.0.1001");
    assert!(win.url.ends_with("1001.Windows.zip"));
    assert_eq!(
        launcher_release(&releases, ".linux.tar.gz")
            .unwrap()
            .version,
        "1.0.0.1002"
    );
    assert_eq!(plugin_release(&releases).unwrap().version, "v1.0.0.1995");

    let newer = |version: &str| Release {
        version: version.into(),
        url: String::new(),
        digest: None,
    };
    assert!(newer("99.0.0.0").is_newer_launcher());
    assert!(!newer("0.0.0.1").is_newer_launcher());
    assert!(!newer(&launcher_version().map(|p| p.to_string()).join(".")).is_newer_launcher());
}

#[test]
fn launcher_archives() {
    let mut zip_data = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut zip_data));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("readme.txt", options).unwrap();
        zip.write_all(b"text").unwrap();
        zip.start_file("NFSUServerChanger.exe", options).unwrap();
        zip.write_all(b"MZ new").unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(
        binary_from_zip(&zip_data, "NFSUServerChanger.exe").unwrap(),
        b"MZ new"
    );
    assert!(binary_from_zip(&zip_data, "other.exe").is_err());

    let mut tar_gz = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(8);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "NFSU/NFSUServerChanger", &b"ELF new!"[..])
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    assert_eq!(
        binary_from_tar_gz(&tar_gz, "NFSUServerChanger").unwrap(),
        b"ELF new!"
    );
    assert!(binary_from_tar_gz(&tar_gz, "other").is_err());
}

#[test]
fn replaces_binary() {
    let dir = std::env::temp_dir().join(format!("nfsu-sel-self-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("NFSUServerChanger.exe");
    fs::write(&exe, b"old").unwrap();
    // A leftover from an earlier update doesn't get in the way.
    fs::write(dir.join("NFSUServerChanger.exe.old"), b"older").unwrap();
    replace_binary(&exe, b"new").unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"new");
    assert_eq!(
        fs::read(dir.join("NFSUServerChanger.exe.old")).unwrap(),
        b"old"
    );
    assert!(!dir.join("NFSUServerChanger.exe.new").exists());
    fs::remove_dir_all(&dir).unwrap();
}
