// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod i18n;
mod ini;
mod installer;
mod launcher;
#[cfg(not(windows))]
mod lutris;
#[cfg(not(windows))]
mod ping;
mod servers;
mod settings;
mod single_instance;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use i18n::{t, tf};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

const INI_DIR: &str = "scripts";
const INI_NAME: &str = "NFSUServerChanger.ini";
const CUSTOM_LIST_NAME: &str = "servers.dat";
const TRAX_NAME: &str = "NFSUServerChangerTrax.csv";
// Template for the trax list: the game's original titles, same as the plugin's example.
const TRAX_TEMPLATE: &str = include_str!("../../plugin/NFSUServerChangerTrax.csv");
const PLUGIN_NAME: &str = "NFSUServerChanger.asi";
// Downloaded translations, in the scripts folder.
const LANG_DIR: &str = "lang";
const ICON_NAME: &str = "NFSU_icon.ico";
const PROJECT_URL: &str = "https://github.com/Pelorojo/NFSUServerChanger";
// The game .exe has a 32-byte slot for the host (the lobby port follows right after it),
// so 31 chars + terminating zero; NFSUServerChanger cuts it off there (dllmain.cpp).
const MAX_HOST_LEN: usize = 31;
// Room for an optional ":port" (lobby port, see servers::split_host_port) after the name.
const MAX_INPUT_LEN: usize = MAX_HOST_LEN + ":65535".len();

/// Files the app reads/writes, all in the game folder.
#[derive(Clone)]
struct Files {
    game_dir: PathBuf,
    ini: Option<PathBuf>,
    /// The plugin's EA Trax list, next to the ini (it may not exist yet).
    trax: Option<PathBuf>,
    /// The plugin itself; without it the game never reads the ini.
    plugin: Option<PathBuf>,
    /// speed*.exe in the game folder.
    games: Vec<PathBuf>,
    custom: PathBuf,
    cache: PathBuf,
    /// scripts/lang, for the translations (it may not exist yet).
    lang: PathBuf,
}

impl Files {
    fn locate() -> Self {
        let ini = find_ini();
        // The game folder holds the ini's "scripts" folder; without an ini,
        // fall back to the folder of the executable.
        let game_dir = ini
            .as_deref()
            .and_then(|p| p.parent()?.parent())
            .map(Path::to_path_buf)
            .or_else(exe_dir)
            .unwrap_or_default();
        let file = |name: &str| find_ci(&game_dir, name).unwrap_or_else(|| game_dir.join(name));
        let trax = ini.as_deref().and_then(|p| {
            let scripts = p.parent()?;
            Some(find_ci(scripts, TRAX_NAME).unwrap_or_else(|| scripts.join(TRAX_NAME)))
        });
        let scripts = find_ci(&game_dir, INI_DIR).unwrap_or_else(|| game_dir.join(INI_DIR));
        let lang = find_ci(&scripts, LANG_DIR).unwrap_or_else(|| scripts.join(LANG_DIR));
        let plugin = find_ci(&scripts, PLUGIN_NAME);
        Files {
            games: launcher::find_games(&game_dir),
            trax,
            plugin,
            custom: file(CUSTOM_LIST_NAME),
            cache: file(servers::CACHE_NAME),
            lang,
            ini,
            game_dir,
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    // Only one launcher: a second start brings the running one to front and quits.
    let instance = match single_instance::claim() {
        single_instance::Claim::Primary(guard) => guard,
        single_instance::Claim::Secondary => return Ok(()),
    };
    // Also takes the launcher's path before an update could rename the file.
    installer::clean_up_launcher_update();
    let app = AppWindow::new()?;
    instance.serve({
        let weak = app.as_weak();
        move || {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    bring_to_front(&app);
                }
            });
        }
    });
    // Shared, so installing the plugin can re-locate everything without a restart.
    let files = Rc::new(RefCell::new(Files::locate()));
    if let Some(icon) = find_ci(&files.borrow().game_dir, ICON_NAME).and_then(|p| load_icon(&p)) {
        app.set_window_icon(icon);
    }
    let custom = Rc::new(RefCell::new(read_custom_list(&files.borrow().custom)));

    // --- Language ---
    // The saved choice, else the system's language; English if there's no such file.
    let lang_dir = Some(files.borrow().lang.clone());
    let saved = settings::language(files.borrow().ini.as_deref());
    let wanted = if saved.is_empty() {
        i18n::system_language()
    } else {
        saved
    };
    let current = if i18n::set(lang_dir.as_deref(), &wanted) {
        wanted
    } else {
        i18n::set(None, "en");
        "en".to_string()
    };
    let tr = app.global::<Tr>();
    tr.on_get(|key, _| t(&key).into());
    tr.on_fmt(|key, arg, _| tf(&key, &[&arg]).into());
    let languages = Rc::new(VecModel::<LanguageItem>::default());
    app.set_languages(ModelRc::from(languages.clone()));
    let fill_languages = {
        let languages = languages.clone();
        let lang_dir = lang_dir.clone();
        move |current: &str| {
            languages.set_vec(
                i18n::available(lang_dir.as_deref())
                    .into_iter()
                    .map(|l| LanguageItem {
                        current: l.code.eq_ignore_ascii_case(current),
                        code: l.code.into(),
                        name: menu_text(&l.name).into(),
                    })
                    .collect::<Vec<_>>(),
            )
        }
    };
    fill_languages(&current);
    app.on_select_language({
        let weak = app.as_weak();
        let files = files.clone();
        let lang_dir = lang_dir.clone();
        move |code| {
            let app = weak.unwrap();
            if !i18n::set(lang_dir.as_deref(), &code) {
                return;
            }
            settings::set_language(files.borrow().ini.as_deref(), &code);
            fill_languages(&code);
            // Every text binding in the UI depends on the revision.
            let tr = app.global::<Tr>();
            tr.set_revision(tr.get_revision() + 1);
            // Texts made in Rust.
            let files = files.borrow();
            app.set_problem(problem_text(&files).into());
            app.set_host_error(
                validate_host(app.get_host_input().trim())
                    .err()
                    .unwrap_or_default()
                    .into(),
            );
            app.set_status(status_text().into());
            app.set_message("".into());
        }
    });

    // Download popup: the translations on GitHub, fetched each time it opens.
    let online = Arc::new(Mutex::new(Vec::<i18n::OnlineLanguage>::new()));
    app.on_open_languages({
        let weak = app.as_weak();
        let online = online.clone();
        let lang_dir = lang_dir.clone();
        move || {
            let app = weak.unwrap();
            if app.get_langs_loading() {
                return;
            }
            app.set_langs_loading(true);
            app.set_langs_error("".into());
            app.set_lang_packages(ModelRc::default());
            let weak = app.as_weak();
            let online = online.clone();
            let lang_dir = lang_dir.clone();
            std::thread::spawn(move || {
                let result = i18n::online_languages();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = weak.upgrade() else { return };
                    app.set_langs_loading(false);
                    match result {
                        Ok(langs) => {
                            let mut online = online.lock().unwrap_or_else(|e| e.into_inner());
                            *online = langs;
                            app.set_lang_packages(lang_packages(&online, lang_dir.as_deref()));
                        }
                        Err(e) => app.set_langs_error(e.to_string().into()),
                    }
                });
            });
        }
    });
    // Installing (or updating) switches to that language right away.
    app.on_install_language({
        let weak = app.as_weak();
        let online = online.clone();
        let lang_dir = lang_dir.clone();
        move |code| {
            let app = weak.unwrap();
            let Some(dir) = lang_dir.as_deref() else {
                return;
            };
            let online = online.lock().unwrap_or_else(|e| e.into_inner());
            let Some(lang) = online.iter().find(|l| l.code == code.as_str()) else {
                return;
            };
            match i18n::install(dir, lang) {
                Ok(()) => {
                    app.set_lang_packages(lang_packages(&online, Some(dir)));
                    app.invoke_select_language(code);
                }
                Err(e) => app.set_message(
                    tf(
                        "msg.could_not_write",
                        &[&dir.display().to_string(), &e.to_string()],
                    )
                    .into(),
                ),
            }
        }
    });
    // Removing the current language goes back to English.
    app.on_remove_language({
        let weak = app.as_weak();
        move |code| {
            let app = weak.unwrap();
            let Some(dir) = lang_dir.as_deref() else {
                return;
            };
            let current = app
                .get_languages()
                .iter()
                .find(|l| l.current)
                .map_or_else(|| "en".into(), |l| l.code);
            match i18n::remove(dir, &code) {
                Ok(()) => {
                    let online = online.lock().unwrap_or_else(|e| e.into_inner());
                    app.set_lang_packages(lang_packages(&online, Some(dir)));
                    let next = if current.eq_ignore_ascii_case(&code) {
                        "en".into()
                    } else {
                        current
                    };
                    app.invoke_select_language(next);
                }
                Err(e) => app.set_message(
                    tf(
                        "msg.could_not_write",
                        &[&dir.display().to_string(), &e.to_string()],
                    )
                    .into(),
                ),
            }
        }
    });

    // Re-reads the files after the plugin was installed.
    app.on_reload({
        let weak = app.as_weak();
        let files = files.clone();
        let custom = custom.clone();
        move || {
            let app = weak.unwrap();
            *files.borrow_mut() = Files::locate();
            load_state(&app, &files.borrow(), &custom.borrow());
            start_fetch(app.as_weak(), custom.borrow().clone(), &files.borrow());
        }
    });

    // --- Plugin and launcher updates from GitHub ---
    // Nothing is installed by itself: at start (quietly) and via menu "Check for updates",
    // what's newer is offered in the updates popup.
    check_for_updates(&app, &files.borrow(), true);
    app.on_install_plugin({
        let weak = app.as_weak();
        let files = files.clone();
        move || install_plugin(&weak.unwrap(), &files.borrow().game_dir)
    });
    app.on_check_for_updates({
        let weak = app.as_weak();
        let files = files.clone();
        move || check_for_updates(&weak.unwrap(), &files.borrow(), false)
    });
    // "Update" in the updates popup: install, then restart into the new launcher.
    app.on_update_launcher({
        let weak = app.as_weak();
        move || {
            let app = weak.unwrap();
            let version = app.get_launcher_update().to_string();
            if version.is_empty() {
                return;
            }
            app.set_message(tf("msg.downloading_launcher", &[&version]).into());
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let result = installer::check()
                    .and_then(|updates| {
                        let release = updates
                            .launcher
                            .filter(installer::Release::is_newer_launcher)
                            .ok_or("no newer launcher release")?;
                        installer::update_launcher(&release)?;
                        Ok(release.version)
                    })
                    .map_err(|e| e.to_string());
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = weak.upgrade() else { return };
                    match result {
                        Ok(version) => show_launcher_update(&app, version, Some(Ok(()))),
                        Err(e) => app.set_message(tf("msg.launcher_update_failed", &[&e]).into()),
                    }
                });
            });
        }
    });
    app.on_restart_launcher({
        let weak = app.as_weak();
        move || match installer::restart_launcher() {
            Ok(()) => {
                let _ = slint::quit_event_loop();
            }
            Err(e) => weak.unwrap().set_message(
                tf(
                    "msg.could_not_start",
                    &["NFSUServerChanger", &e.to_string()],
                )
                .into(),
            ),
        }
    });

    app.on_refresh({
        let weak = app.as_weak();
        let custom = custom.clone();
        let files = files.clone();
        move || start_fetch(weak.clone(), custom.borrow().clone(), &files.borrow())
    });

    app.on_host_changed({
        let weak = app.as_weak();
        let custom = custom.clone();
        move |host| {
            let app = weak.unwrap();
            // LineEdit has no max length: cut it here (this handler runs again for the cut text).
            if host.chars().count() > MAX_INPUT_LEN {
                app.set_host_input(host.chars().take(MAX_INPUT_LEN).collect::<String>().into());
                return;
            }
            app.set_input_is_custom(contains_host(&custom.borrow(), host.trim()));
            app.set_host_error(validate_host(host.trim()).err().unwrap_or_default().into());
        }
    });

    app.on_add_custom({
        let weak = app.as_weak();
        let custom = custom.clone();
        let files = files.clone();
        move |host| {
            let app = weak.unwrap();
            let host = host.trim();
            if let Err(msg) = validate_host(host) {
                app.set_message(msg.into());
                return;
            }
            // A bare ":port" (default server on another port) is no server to list.
            if servers::split_host_port(host).0.is_empty() || contains_host(&custom.borrow(), host)
            {
                return;
            }
            let mut list = custom.borrow().clone();
            list.push(host.to_string());
            save_custom_list(&app, &custom, &files.borrow(), list);
        }
    });

    app.on_remove_custom({
        let weak = app.as_weak();
        let custom = custom.clone();
        let files = files.clone();
        move |host| {
            let app = weak.unwrap();
            let host = host.trim();
            let list: Vec<String> = custom
                .borrow()
                .iter()
                .filter(|h| !h.eq_ignore_ascii_case(host))
                .cloned()
                .collect();
            save_custom_list(&app, &custom, &files.borrow(), list);
        }
    });

    app.on_apply({
        let weak = app.as_weak();
        let files = files.clone();
        move |host| {
            apply_host(&weak.unwrap(), &files.borrow(), host.trim());
        }
    });

    // --- Menu ---
    let open_in_system = {
        let weak = app.as_weak();
        move |target: &str| {
            if let Err(e) = launcher::open(target) {
                weak.unwrap()
                    .set_message(tf("msg.could_not_open", &[target, &e.to_string()]).into());
            }
        }
    };
    app.on_open_game_folder({
        let open = open_in_system.clone();
        let files = files.clone();
        move || open(&files.borrow().game_dir.to_string_lossy())
    });
    // Opens a file in the editor from the settings.
    let edit_file = {
        let weak = app.as_weak();
        let files = files.clone();
        move |path: &Path| {
            if let Err(e) = launcher::edit(path, &settings::editor(files.borrow().ini.as_deref())) {
                weak.unwrap().set_message(
                    tf(
                        "msg.could_not_open",
                        &[&path.display().to_string(), &e.to_string()],
                    )
                    .into(),
                );
            }
        }
    };
    app.on_edit_ini({
        let edit = edit_file.clone();
        let files = files.clone();
        move || {
            let ini = files.borrow().ini.clone();
            if let Some(ini) = ini {
                edit(&ini);
            }
        }
    });
    app.on_edit_trax({
        let edit = edit_file.clone();
        let files = files.clone();
        move || {
            let trax = files.borrow().trax.clone();
            if let Some(trax) = trax {
                // Not shipped with the plugin release: created from the launcher's built-in
                // template (the game's original track list) the first time it's edited.
                if !trax.exists() {
                    let _ = fs::write(&trax, TRAX_TEMPLATE);
                }
                edit(&trax);
            }
        }
    });
    app.on_edit_custom_list({
        let edit = edit_file.clone();
        let files = files.clone();
        move || {
            let path = files.borrow().custom.clone();
            // Create it empty first, so there's something to open.
            if !path.exists() {
                let _ = fs::write(&path, "");
            }
            edit(&path);
        }
    });

    // Settings popup: filled from the ini when opened, written back on Save.
    let public_lists = Rc::new(VecModel::<PublicList>::default());
    app.set_settings_public_lists(ModelRc::from(public_lists.clone()));
    app.on_open_settings({
        let weak = app.as_weak();
        let files = files.clone();
        let public_lists = public_lists.clone();
        move || {
            let app = weak.unwrap();
            let ini = files.borrow().ini.clone();
            app.set_settings_editor(settings::editor(ini.as_deref()).into());
            app.set_settings_command(settings::launch_command(ini.as_deref()).into());
            app.set_settings_launch_info(launcher::launch_info(&files.borrow().game_dir).into());
            #[cfg(not(windows))]
            {
                let wine = launcher::game_wine(
                    &files.borrow().game_dir,
                    &settings::launch_command(ini.as_deref()),
                );
                let (info, can_enable) = match ping::state(wine.as_deref()) {
                    ping::State::On => (t("settings.ping_on"), false),
                    ping::State::Off => (t("settings.ping_off"), true),
                    ping::State::OldWine(version) => {
                        (tf("settings.ping_old_wine", &[&version]), false)
                    }
                };
                app.set_settings_ping_info(info.into());
                app.set_settings_ping_can_enable(can_enable);
            }
            public_lists.set_vec(
                settings::public_lists(ini.as_deref())
                    .into_iter()
                    .map(|(name, url, enabled)| PublicList {
                        name: name.into(),
                        url: url.into(),
                        enabled,
                    })
                    .collect::<Vec<_>>(),
            );
        }
    });
    app.on_toggle_public_list({
        let public_lists = public_lists.clone();
        move |index, enabled| {
            let index = index as usize;
            if let Some(mut row) = public_lists.row_data(index) {
                row.enabled = enabled;
                public_lists.set_row_data(index, row);
            }
        }
    });
    app.on_save_settings({
        let weak = app.as_weak();
        let custom = custom.clone();
        let files = files.clone();
        let public_lists = public_lists.clone();
        move || {
            let app = weak.unwrap();
            let files = files.borrow();
            let Some(ini) = &files.ini else { return };
            let enabled: Vec<String> = public_lists
                .iter()
                .filter(|l| l.enabled)
                .map(|l| l.name.to_string())
                .collect();
            match settings::save(ini, app.get_settings_editor().trim(), &enabled)
                .and_then(|()| settings::set_launch_command(ini, app.get_settings_command().trim()))
            {
                // The public lists may have changed.
                Ok(()) => start_fetch(app.as_weak(), custom.borrow().clone(), &files),
                Err(e) => app.set_message(
                    tf(
                        "msg.could_not_write",
                        &[&ini.display().to_string(), &e.to_string()],
                    )
                    .into(),
                ),
            }
        }
    });
    app.on_open_url({
        let open = open_in_system.clone();
        move |url| open(&url)
    });
    app.on_open_project_page({
        let open = open_in_system.clone();
        move || open(PROJECT_URL)
    });
    app.set_version(launcher_version().into());
    app.on_exit(|| {
        let _ = slint::quit_event_loop();
    });

    // The chosen exe is remembered right away, not only when it's started.
    app.on_game_selected({
        let files = files.clone();
        move |name| settings::set_last_game(files.borrow().ini.as_deref(), &name)
    });

    app.on_start_game({
        let weak = app.as_weak();
        let files = files.clone();
        move |index| {
            let app = weak.unwrap();
            let files = files.borrow();
            let Some(exe) = usize::try_from(index).ok().and_then(|i| files.games.get(i)) else {
                return;
            };
            // The game only once: a second copy would fight the first one over the ini
            // and the network ports.
            let names: Vec<String> = files
                .games
                .iter()
                .map(|g| {
                    g.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            if let Some(running) = launcher::running_game(&names) {
                app.set_message(tf("msg.game_running", &[&running]).into());
                return;
            }
            // Whatever is in the field is what the game connects to.
            if files.ini.is_some() && !apply_host(&app, &files, app.get_host_input().trim()) {
                return;
            }
            let command = settings::launch_command(files.ini.as_deref());
            let name = exe.file_name().unwrap_or_default().to_string_lossy();
            match launcher::launch(exe, &files.game_dir, &command) {
                Ok(()) => app.set_message(tf("msg.started", &[&name]).into()),
                Err(e) => {
                    app.set_message(tf("msg.could_not_start", &[&name, &e.to_string()]).into())
                }
            }
        }
    });

    // Windows: a game in e.g. C:\Program Files can't be written to without admin rights,
    // so the ini, servers.dat and plugin updates would fail. Offer a restart as admin.
    #[cfg(windows)]
    {
        let files = files.borrow();
        let in_game_folder = files.ini.is_some() || !files.games.is_empty();
        let scripts_writable = files
            .ini
            .as_deref()
            .and_then(Path::parent)
            .is_none_or(launcher::dir_writable);
        app.set_needs_admin(
            in_game_folder && !(launcher::dir_writable(&files.game_dir) && scripts_writable),
        );
    }
    // --- Pings (Linux) ---
    // Without ping sockets for the user's groups the game's pings time out (the plugin keeps
    // it from crashing). Offered at start unless "Don't ask again", and in the settings.
    #[cfg(not(windows))]
    {
        app.set_ping_command(ping::command().into());
        let files = files.borrow();
        if settings::ping_hint(files.ini.as_deref()) {
            let weak = app.as_weak();
            let wine = launcher::game_wine(
                &files.game_dir,
                &settings::launch_command(files.ini.as_deref()),
            );
            std::thread::spawn(move || {
                let off = matches!(ping::state(wine.as_deref()), ping::State::Off);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_ping_offer(off);
                    }
                });
            });
        }
    }
    app.on_enable_ping({
        let weak = app.as_weak();
        move || {
            let app = weak.unwrap();
            app.set_ping_busy(true);
            app.set_ping_error("".into());
            let weak = app.as_weak();
            std::thread::spawn(move || {
                #[cfg(not(windows))]
                let result = ping::enable();
                #[cfg(windows)]
                let result: Result<(), String> = Ok(());
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = weak.upgrade() else { return };
                    app.set_ping_busy(false);
                    match result {
                        Ok(()) => {
                            app.set_ping_offer(false);
                            app.set_message(t("msg.ping_enabled").into());
                        }
                        Err(e) => app.set_ping_error(tf("ping.failed", &[&e]).into()),
                    }
                });
            });
        }
    });
    app.on_ping_dont_ask({
        let weak = app.as_weak();
        let files = files.clone();
        move || {
            let app = weak.unwrap();
            app.set_ping_offer(false);
            if let Some(ini) = &files.borrow().ini {
                if let Err(e) = settings::set_ping_hint(ini, false) {
                    app.set_message(
                        tf(
                            "msg.could_not_write",
                            &[&ini.display().to_string(), &e.to_string()],
                        )
                        .into(),
                    );
                }
            }
        }
    });
    app.on_restart_as_admin({
        let weak = app.as_weak();
        move || {
            #[cfg(windows)]
            match launcher::restart_elevated() {
                Ok(()) => {
                    let _ = slint::quit_event_loop();
                }
                Err(e) => weak
                    .unwrap()
                    .set_message(tf("msg.admin_failed", &[&e.to_string()]).into()),
            }
            #[cfg(not(windows))]
            let _ = &weak;
        }
    });

    load_state(&app, &files.borrow(), &custom.borrow());
    start_fetch(app.as_weak(), custom.borrow().clone(), &files.borrow());
    app.run()
}

/// Everything shown that comes from the files: current host, plugin warning, games,
/// community links. At start and again after the plugin was installed.
fn load_state(app: &AppWindow, files: &Files, custom: &[String]) {
    let host = files
        .ini
        .as_deref()
        .and_then(|p| ini::read_value(p, "Server", "Host"))
        .unwrap_or_default();
    app.set_ini_found(files.ini.is_some());
    app.set_current_host(host.clone().into());
    app.set_host_input(host.into());
    app.set_input_is_custom(contains_host(custom, app.get_host_input().trim()));
    app.set_trax_found(files.trax.is_some());

    app.set_problem(problem_text(files).into());
    app.set_can_install(!files.games.is_empty() || files.ini.is_some());

    let game_names: Vec<SharedString> = files
        .games
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref()
                .into()
        })
        .collect();
    // Preselect the game started last time.
    let last_game = settings::last_game(files.ini.as_deref());
    let game_index = game_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(&last_game))
        .unwrap_or(0);
    app.set_games(ModelRc::from(Rc::new(VecModel::from(game_names))));
    app.set_game_index(game_index as i32);

    let links: Vec<Link> = settings::community_links(files.ini.as_deref())
        .into_iter()
        .map(|(name, url)| Link {
            name: menu_text(&name).into(),
            url: url.into(),
        })
        .collect();
    app.set_community_links(ModelRc::from(Rc::new(VecModel::from(links))));

    let help: Vec<Link> = settings::help_links(files.ini.as_deref())
        .into_iter()
        .map(|(name, url)| Link {
            name: menu_text(&name).into(),
            url: url.into(),
        })
        .collect();
    app.set_help_links(ModelRc::from(Rc::new(VecModel::from(help))));
}

/// Shows the window again when another start asked for it: out of the taskbar and in front.
fn bring_to_front(app: &AppWindow) {
    app.window().set_minimized(false);
    let _ = app.window().show();
    // On Windows the second start raises the window itself (only the foreground app may).
    #[cfg(not(windows))]
    {
        use slint::winit_030::WinitWindowAccessor;
        app.window()
            .with_winit_window(|window| window.focus_window());
    }
}

/// Menu titles on Windows are native menus, where `&` marks the access key and is
/// hidden; `&&` shows a literal `&`. Slint's own menus (Linux) show it as is.
/// The download popup's rows: which of the translations on GitHub are installed / changed.
fn lang_packages(online: &[i18n::OnlineLanguage], dir: Option<&Path>) -> ModelRc<LangPackage> {
    let list: Vec<LangPackage> = online
        .iter()
        .map(|l| {
            let local = dir.and_then(|d| i18n::installed_text(d, &l.code));
            LangPackage {
                code: l.code.as_str().into(),
                name: l.name.as_str().into(),
                installed: local.is_some(),
                update: local.is_some_and(|text| text != l.text),
            }
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(list)))
}

/// Shown instead of the current server: without the plugin the game never reads the ini.
fn problem_text(files: &Files) -> String {
    let plugin = format!("{INI_DIR}{}{PLUGIN_NAME}", std::path::MAIN_SEPARATOR);
    match (&files.ini, &files.plugin) {
        (None, _) if files.games.is_empty() => t("problem.not_in_game_folder"),
        (None, _) => tf("problem.plugin_not_installed", &[&plugin, INI_NAME]),
        (Some(_), None) => tf("problem.plugin_missing", &[&plugin]),
        (Some(_), Some(_)) => String::new(),
    }
}

fn menu_text(text: &str) -> String {
    if cfg!(windows) {
        text.replace('&', "&&")
    } else {
        text.to_string()
    }
}

fn launcher_version() -> String {
    installer::launcher_version()
        .map(|p| p.to_string())
        .join(".")
}

/// Looks for newer plugin/launcher releases and opens the updates popup if there are any.
/// `quiet` (at start): no messages for "up to date" or a failed check.
fn check_for_updates(app: &AppWindow, files: &Files, quiet: bool) {
    if !quiet {
        app.set_message(t("msg.checking_updates").into());
    }
    let weak = app.as_weak();
    let plugin = files.plugin.clone();
    // Without an installed plugin the "Install plugin" button is there anyway.
    let installed = plugin.is_some();
    std::thread::spawn(move || {
        let result = installer::check();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = weak.upgrade() else { return };
            let updates = match result {
                Ok(updates) => updates,
                Err(e) => {
                    if !quiet {
                        app.set_message(tf("msg.update_check_failed", &[&e.to_string()]).into());
                    }
                    return;
                }
            };
            let plugin_release = updates
                .plugin
                .filter(|r| installed && r.is_newer_than(plugin.as_deref()));
            app.set_plugin_update(plugin_release.map(|r| r.version).unwrap_or_default().into());
            // Already installed and waiting for the restart: nothing to offer.
            let launcher_release = updates
                .launcher
                .filter(installer::Release::is_newer_launcher)
                .filter(|r| r.version != app.get_launcher_restart().as_str());
            app.set_launcher_update(
                launcher_release
                    .map(|r| r.version)
                    .unwrap_or_default()
                    .into(),
            );
            if app.get_plugin_update().is_empty() && app.get_launcher_update().is_empty() {
                if !quiet {
                    app.set_message(t("msg.up_to_date").into());
                }
            } else {
                if !quiet {
                    app.set_message("".into());
                }
                app.invoke_show_updates();
            }
        });
    });
}

/// A newer launcher: installed (`result` Ok: restart into it, after a running plugin
/// installation), failed, or only offered (`result` None).
fn show_launcher_update(app: &AppWindow, version: String, result: Option<Result<(), String>>) {
    match result {
        Some(Ok(())) => {
            app.set_message(tf("msg.launcher_updated", &[&version]).into());
            app.set_launcher_update("".into());
            app.set_launcher_restart(version.into());
            if !app.get_installing() {
                app.invoke_restart_launcher();
            }
        }
        Some(Err(e)) => {
            app.set_message(tf("msg.launcher_update_failed", &[&e]).into());
            app.set_launcher_update(version.into());
        }
        None => app.set_launcher_update(version.into()),
    }
}

/// Downloads the latest plugin release into the game folder, then reloads the launcher's
/// state from the installed files.
fn install_plugin(app: &AppWindow, game_dir: &Path) {
    if app.get_installing() {
        return;
    }
    app.set_installing(true);
    app.set_message(t("msg.downloading_plugin").into());
    let weak = app.as_weak();
    let game_dir = game_dir.to_path_buf();
    std::thread::spawn(move || {
        let result = installer::latest_release().and_then(|release| {
            installer::install(&release, &game_dir)?;
            Ok(release.version)
        });
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = weak.upgrade() else { return };
            match result {
                Ok(version) => {
                    app.set_installing(false);
                    app.set_plugin_update("".into());
                    // Read everything again from the installed plugin's files.
                    app.invoke_reload();
                    app.set_message(tf("msg.plugin_installed", &[&version]).into());
                }
                Err(e) => {
                    app.set_installing(false);
                    // E.g. the .asi is locked while the game runs.
                    app.set_message(tf("msg.install_failed", &[&e.to_string()]).into());
                }
            }
            // A launcher update waited for this installation.
            if !app.get_launcher_restart().is_empty() {
                app.invoke_restart_launcher();
            }
        });
    });
}

/// Writes the host to NFSUServerChanger.ini (no-op if unchanged). Returns false on error.
fn apply_host(app: &AppWindow, files: &Files, host: &str) -> bool {
    let Some(path) = &files.ini else { return false };
    if let Err(msg) = validate_host(host) {
        app.set_message(msg.into());
        return false;
    }
    if app.get_current_host() == host {
        return true;
    }
    match ini::write_value(path, "Server", "Host", host) {
        Ok(()) => {
            app.set_current_host(host.into());
            true
        }
        Err(e) => {
            app.set_message(
                tf(
                    "msg.could_not_write",
                    &[&path.display().to_string(), &e.to_string()],
                )
                .into(),
            );
            false
        }
    }
}

/// Loads the game's own icon (not embedded in the exe). Loaded by path on purpose:
/// Slint (1.18) only hands the window icon to the window when its image cache key
/// changes, and in-memory images (Image::from_rgba8) have none, so they'd never show.
fn load_icon(path: &Path) -> Option<slint::Image> {
    slint::Image::load_from_path(path).ok()
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Looks for scripts/NFSUServerChanger.ini next to the executable, then in the
/// working directory. Case-insensitive so it also works on Linux file systems.
fn find_ini() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();
    [exe_dir(), cwd]
        .into_iter()
        .flatten()
        .find_map(|dir| find_ci(&find_ci(&dir, INI_DIR)?, INI_NAME))
}

fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// An IPv4 address or a hostname (RFC 1123 labels), optionally with ":port" for the lobby
/// port; empty = the game's default server.
fn validate_host(input: &str) -> Result<(), String> {
    let (host, port) = servers::split_host_port(input);
    // Not advertised anywhere on purpose: an invalid ":port" gets the ordinary message.
    if port.is_none() && input.contains(':') {
        return Err(t("host.invalid_chars"));
    }
    if host.is_empty() {
        // ":port" alone = the game's default server on another port.
        return Ok(());
    }
    if host.len() > MAX_HOST_LEN {
        return Err(tf("host.too_long", &[&MAX_HOST_LEN.to_string()]));
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(t("host.invalid_chars"));
    }
    let labels: Vec<&str> = host.split('.').collect();
    // Digits and dots only: has to be a valid IPv4 address (NFSU only speaks IPv4).
    if labels
        .iter()
        .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
    {
        return host
            .parse::<std::net::Ipv4Addr>()
            .map(|_| ())
            .map_err(|_| t("host.invalid_ip"));
    }
    for label in labels {
        if label.is_empty() {
            return Err(t("host.empty_part"));
        }
        if label.len() > 63 {
            return Err(t("host.label_too_long"));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(t("host.dash"));
        }
    }
    Ok(())
}

// --- Custom server list -----------------------------------------------------
// servers.dat: one IP or hostname per line.

fn contains_host(list: &[String], host: &str) -> bool {
    list.iter().any(|h| h.eq_ignore_ascii_case(host))
}

fn read_custom_list(path: &Path) -> Vec<String> {
    let text = String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned();
    let mut list: Vec<String> = Vec::new();
    for host in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !contains_host(&list, host) {
            list.push(host.to_string());
        }
    }
    list
}

fn write_custom_list(path: &Path, list: &[String]) -> std::io::Result<()> {
    // Keep the file's line endings; new files get the platform's.
    let existing = fs::read(path).unwrap_or_default();
    let eol = if existing.windows(2).any(|w| w == b"\r\n") || (existing.is_empty() && cfg!(windows))
    {
        "\r\n"
    } else {
        "\n"
    };
    let out: String = list.iter().map(|h| format!("{h}{eol}")).collect();
    fs::write(path, out)
}

fn save_custom_list(
    app: &AppWindow,
    custom: &RefCell<Vec<String>>,
    files: &Files,
    list: Vec<String>,
) {
    if let Err(e) = write_custom_list(&files.custom, &list) {
        app.set_message(
            tf(
                "msg.could_not_write",
                &[&files.custom.display().to_string(), &e.to_string()],
            )
            .into(),
        );
        return;
    }
    *custom.borrow_mut() = list;
    app.set_input_is_custom(contains_host(&custom.borrow(), app.get_host_input().trim()));
    start_fetch(app.as_weak(), custom.borrow().clone(), files);
}

// --- Fetching ---------------------------------------------------------------

// Bumped on every fetch, so results of an outdated fetch are dropped.
static FETCH_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Shows the cached state right away, then queries every server in parallel
/// and updates the list as each answer comes in.
fn start_fetch(weak: slint::Weak<AppWindow>, custom: Vec<String>, files: &Files) {
    let generation = FETCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let cache_path = files.cache.clone();
    let providers = settings::providers(files.ini.as_deref());
    let fixed_servers = settings::fixed_servers(files.ini.as_deref());
    std::thread::spawn(move || {
        let mut cached = servers::load_cache(&cache_path);
        // Shown right away, but only as "checking" until each one is queried again:
        // the cache may say online for a server that's down by now.
        for entry in &mut cached {
            entry.pending = true;
        }
        let mut known = servers::known_servers(&cached, &fixed_servers, &custom);
        publish(
            &weak,
            generation,
            servers::merge_custom(&known, &custom),
            known.len(),
        );

        // Add what the public lists know; IPs resolved first so hostnames
        // already in the list aren't added a second time by IP.
        servers::resolve_ips(&mut known);
        let public = servers::fetch_public_lists(&providers);
        let public_ok = public.is_some();
        servers::add_public(&mut known, public.as_deref());
        let mut remaining = known.len();
        publish(
            &weak,
            generation,
            servers::merge_custom(&known, &custom),
            remaining,
        );

        let (tx, rx) = mpsc::channel();
        for (i, entry) in known.iter().cloned().enumerate() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((i, servers::probe(entry)));
            });
        }
        drop(tx);
        for (i, entry) in rx {
            known[i] = entry;
            remaining -= 1;
            publish(
                &weak,
                generation,
                servers::merge_custom(&known, &custom),
                remaining,
            );
        }
        // A newer fetch (Refresh) may have started meanwhile; only the latest writes the cache.
        if generation == FETCH_GENERATION.load(Ordering::SeqCst) {
            servers::save_cache(&cache_path, &known, public_ok);
        }
    });
}

fn publish(weak: &slint::Weak<AppWindow>, generation: u64, list: Vec<Server>, remaining: usize) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if generation != FETCH_GENERATION.load(Ordering::SeqCst) {
            return;
        }
        let Some(app) = weak.upgrade() else { return };
        app.set_loading(remaining > 0);
        let online = list.iter().filter(|s| s.reachable && !s.checking).count();
        STATUS_COUNTS.set((online, remaining));
        set_list(&app, list, status_text());
    });
}

thread_local! {
    // (online, still checking) of the last published list, for the status line.
    static STATUS_COUNTS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

fn status_text() -> String {
    let (online, remaining) = STATUS_COUNTS.get();
    let mut status = tf("status.online", &[&online.to_string()]);
    if remaining > 0 {
        status += " ";
        status += &tf("status.checking", &[&remaining.to_string()]);
    }
    status
}

fn set_list(app: &AppWindow, list: Vec<Server>, status: String) {
    app.set_status(status.into());
    // Keep the highlighted row on the same server when the list reorders.
    let input = app.get_host_input();
    let selected = list
        .iter()
        .position(|s| s.host == input)
        .map_or(-1, |i| i as i32);
    app.set_servers(ModelRc::from(Rc::new(VecModel::from(list))));
    app.set_selected(selected);
}

#[cfg(test)]
#[path = "tests/main.rs"]
mod tests;
