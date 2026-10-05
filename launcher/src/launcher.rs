//! Finds and starts the game executable (speed*.exe in the game folder).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// All speed*.exe in `dir` (case-insensitive), speed.exe first.
pub fn find_games(dir: &Path) -> Vec<PathBuf> {
    let mut games: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    p.is_file() && name.starts_with("speed") && name.ends_with(".exe")
                })
                .collect()
        })
        .unwrap_or_default();
    games.sort_by_key(|p| {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        (name != "speed.exe", name)
    });
    games
}

/// Starts a program without waiting for it. A thread waits for its end instead, so it
/// doesn't linger as a zombie process (Linux) while the launcher stays open.
fn spawn(mut cmd: Command) -> std::io::Result<()> {
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Whether files can be created in `dir` (a quick test file, removed again).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".nfsuserverchanger-write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// Starts this program again with administrator rights (the usual UAC prompt).
#[cfg(windows)]
pub fn restart_elevated() -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let exe = std::env::current_exe()?;
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    // Marked as a restart, so it waits for this process to quit instead of handing over to it.
    let params = wide(crate::single_instance::RESTARTED_ARG.as_ref());
    let dir = exe.parent().map(|d| wide(d.as_os_str()));
    // SAFETY: all strings are NUL-terminated UTF-16 and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    // Success is a value above 32; declining the UAC prompt ends up here as an error.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The first of `games` (file names like "Speed1Euro.exe") that is running right now.
pub fn running_game(games: &[String]) -> Option<String> {
    let running = process_names();
    games
        .iter()
        .find(|game| running.iter().any(|p| process_matches(p, game)))
        .cloned()
}

/// Whether a process name belongs to `game`. On Linux the kernel keeps only the first
/// 15 characters of a name, so a cut-off name matches the start of the game's name.
#[cfg_attr(windows, allow(dead_code))]
fn process_matches(process: &str, game: &str) -> bool {
    let (process, game) = (process.to_ascii_lowercase(), game.to_ascii_lowercase());
    process == game || (process.len() == 15 && game.starts_with(&process))
}

/// File names of all running programs.
#[cfg(windows)]
fn process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut names = Vec::new();
    // SAFETY: snapshot handle checked before use and closed at the end; the entry struct
    // is zeroed with its size set as the API requires.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    names
}

/// Names of all running programs: the kernel's process name, plus the program path's file
/// name from the command line - a game under Wine shows up with its .exe there
/// ("C:\\...\\Speed1Euro.exe" or a Unix path).
#[cfg(not(windows))]
fn process_names() -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return names;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        if let Ok(comm) = fs::read_to_string(dir.join("comm")) {
            names.push(comm.trim_end().to_string());
        }
        if let Ok(cmdline) = fs::read(dir.join("cmdline")) {
            let program = cmdline.split(|&b| b == 0).next().unwrap_or_default();
            let program = String::from_utf8_lossy(program);
            if let Some(file) = program.rsplit(['/', '\\']).next() {
                names.push(file.to_string());
            }
        }
    }
    names
}

/// Opens a folder, file or URL with the system's default handler.
#[cfg(not(windows))]
pub fn open(target: &str) -> std::io::Result<()> {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(target);
    spawn(cmd)
}

/// Opens a folder, file or URL with the system's default handler. Not via
/// `explorer <target>`: explorer splits its command line at '=' and ',', so
/// "https://...doku.php?id=tutorials:serverchanger" became "tutorials:serverchanger"
/// and Windows asked for an app for a "tutorials:" link.
#[cfg(windows)]
pub fn open(target: &str) -> std::io::Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("open");
    let file = wide(target);
    // SAFETY: both strings are NUL-terminated UTF-16 and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Success is a value above 32.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn launch(exe: &Path, game_dir: &Path, custom_command: &str) -> std::io::Result<()> {
    let mut cmd = if custom_command.trim().is_empty() {
        default_command(exe)?
    } else {
        shell_command(&expand_command(custom_command, "{exe}", exe))
    };
    cmd.current_dir(game_dir);
    spawn(cmd)
}

/// Opens a file in the configured editor (a command like the launch command,
/// with `{file}`), or in the default text editor if none is set. Going through
/// the file type's default handler instead would ask for a program on Windows
/// for servers.dat and open the .csv in a spreadsheet app.
pub fn edit(file: &Path, editor: &str) -> std::io::Result<()> {
    let editor = editor.trim();
    // "notepad" is the Windows preset; elsewhere the default text editor is the equivalent.
    if editor.is_empty() || (!cfg!(windows) && editor.eq_ignore_ascii_case("notepad")) {
        return spawn(default_text_editor(file));
    }
    spawn(shell_command(&expand_command(editor, "{file}", file)))
}

#[cfg(windows)]
fn default_text_editor(file: &Path) -> Command {
    let mut cmd = Command::new("notepad");
    cmd.arg(file);
    cmd
}

/// The desktop's default application for text/plain, started from its .desktop
/// file's Exec line; xdg-open if there is none.
#[cfg(not(windows))]
fn default_text_editor(file: &Path) -> Command {
    let desktop_command = || -> Option<String> {
        let output = Command::new("xdg-mime")
            .args(["query", "default", "text/plain"])
            .output()
            .ok()?;
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if id.is_empty() {
            return None;
        }
        let entry = desktop_entry(&id)?;
        let exec = desktop_value(&entry, "Exec")?;
        let command = desktop_exec(&exec, file);
        if desktop_value(&entry, "Terminal").is_some_and(|t| t == "true") {
            // Terminal editors (nano, micro, ...) need a terminal window.
            which("x-terminal-emulator")?;
            return Some(format!("x-terminal-emulator -e {command}"));
        }
        Some(command)
    };
    match desktop_command() {
        Some(command) => shell_command(&command),
        None => {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(file);
            cmd
        }
    }
}

/// Contents of a .desktop file, looked up in the XDG data dirs.
#[cfg(not(windows))]
fn desktop_entry(id: &str) -> Option<String> {
    let home = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
        std::env::var("HOME")
            .ok()
            .map(|h| format!("{h}/.local/share"))
    });
    let dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(dirs.split(':').map(String::from))
        .find_map(|dir| std::fs::read_to_string(Path::new(&dir).join("applications").join(id)).ok())
}

/// A key from the [Desktop Entry] group.
#[cfg_attr(windows, allow(dead_code))]
fn desktop_value(entry: &str, key: &str) -> Option<String> {
    let mut in_main = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
        } else if in_main {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Fills an Exec line's field codes: %f/%F/%u/%U become the file, the rest are dropped.
#[cfg_attr(windows, allow(dead_code))]
fn desktop_exec(exec: &str, file: &Path) -> String {
    let mut has_file = false;
    let words: Vec<String> = exec
        .split_whitespace()
        .filter_map(|w| match w {
            "%f" | "%F" | "%u" | "%U" => {
                has_file = true;
                Some(quote(file))
            }
            w if w.starts_with('%') => None,
            w => Some(w.to_string()),
        })
        .collect();
    let mut command = words.join(" ");
    if !has_file {
        command = format!("{command} {}", quote(file));
    }
    command
}

#[cfg(not(windows))]
fn which(program: &str) -> Option<()> {
    std::env::var("PATH")
        .ok()?
        .split(':')
        .any(|dir| Path::new(dir).join(program).is_file())
        .then_some(())
}

/// `placeholder` is replaced by the quoted path; without it the path is appended,
/// so the command also works as a plain prefix (e.g. "gamemoderun wine").
fn expand_command(command: &str, placeholder: &str, path: &Path) -> String {
    if command.contains(placeholder) {
        command.replace(placeholder, &quote(path))
    } else {
        format!("{} {}", command.trim_end(), quote(path))
    }
}

#[cfg(windows)]
fn default_command(exe: &Path) -> std::io::Result<Command> {
    Ok(Command::new(exe))
}

#[cfg(not(windows))]
fn default_command(exe: &Path) -> std::io::Result<Command> {
    let game_dir = exe.parent().unwrap_or(Path::new("."));
    // A Lutris entry for this game folder knows how the game is meant to run.
    let lutris = crate::lutris::find_for(game_dir);
    let Some(wine) = wine_binary(lutris.as_ref()) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            crate::i18n::t("msg.wine_not_found"),
        ));
    };
    let mut cmd = Command::new(wine);
    cmd.arg(exe);
    if let Some(game) = &lutris {
        for (key, value) in &game.env {
            cmd.env(key, value);
        }
    }
    if std::env::var_os("WINEPREFIX").is_none() {
        let prefix = lutris
            .as_ref()
            .and_then(|g| g.prefix.clone())
            .or_else(|| wine_prefix(exe).map(Path::to_path_buf));
        if let Some(prefix) = prefix {
            cmd.env("WINEPREFIX", prefix);
        }
    }
    // Wine prefers its own builtin dinput8/d3d9/... over the game folder's copies, so the
    // ASI loader (dinput8.dll, which loads the plugin) or dgVoodoo/DXVK would be ignored.
    // So every DLL in the game folder gets "native first", plus the Lutris entry's own
    // overrides (which win for the same DLL). An override already in the environment is
    // left alone.
    if std::env::var_os("WINEDLLOVERRIDES").is_none() {
        let lutris_overrides = lutris
            .as_ref()
            .map(|g| g.overrides.as_slice())
            .unwrap_or_default();
        if let Some(overrides) = dll_overrides(game_dir, lutris_overrides) {
            cmd.env("WINEDLLOVERRIDES", overrides);
        }
    }
    Ok(cmd)
}

/// The Lutris entry's Wine if it names one, else the system's `wine`, else the newest
/// Wine Lutris brought along (Lutris doesn't install a system one).
#[cfg(not(windows))]
fn wine_binary(lutris: Option<&crate::lutris::LutrisGame>) -> Option<PathBuf> {
    lutris
        .and_then(|g| g.wine.clone())
        .or_else(|| which("wine").map(|_| PathBuf::from("wine")))
        .or_else(lutris_wine)
}

/// The Wine the game gets started with; None with a custom start command (unknown) or
/// without any Wine.
#[cfg(not(windows))]
pub fn game_wine(game_dir: &Path, custom_command: &str) -> Option<PathBuf> {
    if !custom_command.trim().is_empty() {
        return None;
    }
    wine_binary(crate::lutris::find_for(game_dir).as_ref())
}

/// How the game gets started without a custom command, for the settings dialog.
#[cfg(not(windows))]
pub fn launch_info(game_dir: &Path) -> String {
    let lutris = crate::lutris::find_for(game_dir);
    let wine = match wine_binary(lutris.as_ref()) {
        None => return crate::i18n::t("settings.wine_missing"),
        Some(w) if w == Path::new("wine") => crate::i18n::t("settings.system_wine"),
        Some(w) => format!("{}", w.display()),
    };
    match lutris {
        // A Lutris Wine version that isn't installed falls back to the system's.
        Some(game) if game.wine.is_some() => crate::i18n::tf(
            "settings.launch_lutris",
            &[&game.name, &format!("Wine {}", game.wine_version)],
        ),
        Some(game) => crate::i18n::tf("settings.launch_lutris", &[&game.name, &wine]),
        None => crate::i18n::tf("settings.launch_auto", &[&wine]),
    }
}

#[cfg(windows)]
pub fn launch_info(_game_dir: &Path) -> String {
    String::new()
}

/// What can go into the start command, with examples for this system.
#[cfg(not(windows))]
pub fn command_hint() -> String {
    crate::i18n::t("settings.start_command_hint_linux")
}

#[cfg(windows)]
pub fn command_hint() -> String {
    crate::i18n::t("settings.start_command_hint_windows")
}

/// WINEDLLOVERRIDES for the game folder's DLLs ("native,builtin") merged with `extra`
/// (dll -> mode, e.g. from Lutris), which wins for the same DLL.
#[cfg_attr(windows, allow(dead_code))]
fn dll_overrides(dir: &Path, extra: &[(String, String)]) -> Option<String> {
    let mut modes: std::collections::BTreeMap<String, String> = folder_dlls(dir)
        .into_iter()
        .map(|dll| (dll, "n,b".to_string()))
        .collect();
    for (dll, mode) in extra {
        let dll = dll.to_ascii_lowercase();
        let dll = dll.strip_suffix(".dll").unwrap_or(&dll).to_string();
        modes.insert(dll, short_override(mode));
    }
    if modes.is_empty() {
        return None;
    }
    Some(
        modes
            .iter()
            .map(|(dll, mode)| format!("{dll}={mode}"))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// Lutris spells overrides out ("native,builtin"); WINEDLLOVERRIDES wants "n,b".
#[cfg_attr(windows, allow(dead_code))]
fn short_override(mode: &str) -> String {
    mode.split(',')
        .map(|m| match m.trim().to_ascii_lowercase().as_str() {
            "native" => "n".to_string(),
            "builtin" => "b".to_string(),
            "disabled" => String::new(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Lower-case names (without ".dll") of the DLL files in `dir`, sorted.
#[cfg_attr(windows, allow(dead_code))]
fn folder_dlls(dir: &Path) -> Vec<String> {
    let mut dlls: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    name.strip_suffix(".dll").map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    dlls.sort();
    dlls
}

#[cfg(not(windows))]
fn lutris_wine() -> Option<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))?;
    fs::read_dir(home.join("lutris/runners/wine"))
        .ok()?
        .flatten()
        .map(|e| e.path().join("bin/wine"))
        .filter(|wine| wine.is_file())
        .max_by_key(|wine| fs::metadata(wine).and_then(|m| m.modified()).ok())
}

/// A game inside a wine prefix lives under <prefix>/drive_c/...
#[cfg_attr(windows, allow(dead_code))]
fn wine_prefix(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("drive_c"))
        })?
        .parent()
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("cmd");
    // raw_arg keeps the user's quoting as written. /S + outer quotes: cmd strips exactly
    // those, so commands starting with a quoted path ("C:\Program Files\...") survive.
    cmd.args(["/S", "/C"]).raw_arg(format!("\"{command}\""));
    // No console window flashing up; the started program opens its own window.
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

fn quote(path: &Path) -> String {
    let p = path.to_string_lossy();
    if cfg!(windows) {
        format!("\"{p}\"")
    } else {
        format!("'{}'", p.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
#[path = "tests/launcher.rs"]
mod tests;
