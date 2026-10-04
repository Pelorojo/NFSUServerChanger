# CLAUDE.md

Notes for working on this repository with Claude Code.

## Layout

- `plugin/` - the NFSUServerChanger `.asi` script mod for Need for Speed: Underground (C++,
  Visual Studio, 32-bit). Overwrites the game's lobby server at runtime and renames EA Trax.
- `launcher/` - the launcher `NFSUServerChanger(.exe)` (Rust + Slint, Windows and Linux). Sits in
  the game folder: pick a server, start the game, install/update the plugin.

Both share one config file: `scripts/NFSUServerChanger.ini` in the game folder
(repo copy: `plugin/NFSUServerChanger.ini`). The plugin only reads `[Server]` and `[EATrax]`;
the launcher's sections (`[FixedServers]`, `[PublicList]`, `[Community]`, `[Help]`,
`[Settings]`, `[Launch]`) are ignored by it.

## Building and checking

Launcher (in `launcher/`):

```sh
cargo build --release          # native
cargo test                     # unit tests (src/tests/<module>.rs, included via #[path])
cargo fmt
cargo clippy --release --all-targets
```

- **Always check both platforms** after changes, especially anything behind
  `#[cfg(windows)]` / `#[cfg(not(windows))]` (e.g. the Linux-only `lutris` module):
  on Linux/WSL also run
  `cargo clippy --release --all-targets --target x86_64-pc-windows-gnu`.
  A green Linux build says nothing about the Windows one.
- Windows builds natively with MSVC; Linux builds the Windows `.exe` too via mingw
  (`x86_64-pc-windows-gnu`). The Linux binary can't be built from Windows.
- The release profile uses LTO, so release builds are slow; use debug builds while iterating.
- Keep `cargo clippy` at 0 warnings and code `cargo fmt`-formatted.

Plugin (in `plugin/`): open `NFSUServerChanger.sln` in Visual Studio and build (Win32).
Source files use CRLF; `dllmain.cpp` has a UTF-8 BOM, `NFSUServerChanger.rc` is UTF-16 LE -
keep those encodings when editing.

## Launcher design (decisions already made)

- **No web API.** The server list is built locally: `[FixedServers]` (name = host/IP), the
  trackers in `[PublicList]` (`http://<host>/tracker/get_list.php`, IPs as little-endian u32
  per line; which ones are queried is `[Settings] PublicLists`), `servers.cache` (written by
  the launcher) and `servers.dat` (user's own list, one host per line, shown even when offline).
- Every server is queried directly and in parallel: lobby status on TCP 10980
  (`online|rooms|uptime|system|version|title|...~~~rooms`); servers without it are checked for
  EA Nation via the `@tic` echo on TCP 5000 (12 bytes: `@tic`, 4 reserved, length 12 big-endian).
  IPv4 only (NFSU is IPv4-only; some hostnames have AAAA records the servers don't listen on).
- The INI in `plugin/` is the **single source** of the lists (no defaults in code). INI writes
  touch only the one key, keep comments, line endings and a UTF-8 BOM.
- Selecting a server writes `[Server] Host` right away (no Apply button). Empty Host = keep the
  server built into the .exe (originally `ps2nfs04.ea.com`, but exes may be patched); the
  original EA server is set explicitly as `Host = ps2nfs04.ea.com` (the plugin writes any
  non-empty Host).
- Host limit: **31 characters**. The game has a 32-byte slot for the host, the lobby port
  (10900, u32) follows directly after it.
- `Host = name:port` sets that lobby port too (plugin writes it at host slot + 32). This is
  **intentionally undocumented** for users - don't mention it in the INI, README or UI texts
  (invalid ports get the generic "only letters, digits..." message).
- Plugin install/update: newest GitHub release with an asset named
  `NFSU.Server.Changer.v<version>.zip`; installed version read from the `.asi`'s
  VS_FIXEDFILEINFO. Extraction skips `readme.txt` and `NFSUServerChangerTrax.csv` (only an
  example of the game's own titles; the launcher embeds it as a template and creates it on
  "Edit NFSUServerChangerTrax.csv"), keeps an existing `dinput8.dll`, merges an existing INI (missing keys/sections added; list sections only if missing entirely).
  After installing, the launcher reloads its state (no restart).
- Launcher self-update (`installer.rs`): same release list as the plugin; newest launcher
  asset for the platform, running binary renamed to `.old`, new one put in place (works on
  a running Windows .exe), `.old` deleted at the next start.
- Updates (decided with the user): **no auto update, no setting for it.** At start (quietly)
  and via menu "Check for updates", newer plugin/launcher releases open the updates popup;
  "Update" installs them, and after a launcher update the launcher restarts by itself (after a
  running plugin installation). No update/restart buttons in the main window; progress goes to
  the status line. No conditional menu items: they didn't show up reliably in the native
  Windows menu. Downloads are capped at 100 MB
  (ureq's default of 10 MB is too small for the launcher archives).
- Linux game start with empty `[Launch] Command`: uses the settings of a matching Lutris game
  entry (Wine version, prefix, DLL overrides, env) if one covers the game folder, and loads
  every DLL in the game folder native-first (`dinput8` is the ASI loader - without that
  override the plugin never loads). `Command` accepts `{exe}` or works as a prefix.
- UI: Slint (`launcher/ui/app.slint`), hamburger menu at the **left** edge of the header, settings/about as popups.
  On Linux Slint draws menus inside the window and pushes them back in at the edge; submenus
  open to the right, so a menu at the right edge would get its submenus drawn over it.
  Exit is the last menu item (hamburger convention), not under File. Checkboxes with a
  label use `WrapCheckBox` (Slint's CheckBox text doesn't wrap; translations can be longer).
  On Windows, menu titles are native menus: `&` must be doubled (`menu_text()`).
- Server while the game runs: the plugin hooks the host resolver call of the login
  (0x54D534, reached from the account screens) and re-applies `[Server] Host` from the ini
  there - no thread, no polling. So a server picked in the launcher applies at the next login.
  The other resolver call (0x54990B, from screens shown while logged in) is deliberately not
  hooked: it would mix a running session with another server.
- Pings (Linux, `ping.rs`): the game's raw ICMP ping socket needs `net.ipv4.ping_group_range`
  (and Wine 7.13+, which emulates raw ICMP over ping sockets); without it the game used to
  crash (NULL ping manager, read at 00668791) - the plugin's `PingFix.h` now falls back to a UDP
  socket, so the setting only decides whether pings work. The launcher offers it at start
  (unless `[Settings] PingHint = 0`) and in the settings; it runs the command via `pkexec`
  (never handles the password itself) and only touches `/etc/sysctl.d/99-nfsu-ping.conf`.
  No "open a terminal" option: terminal programs differ per distro.
- Single instance (`single_instance.rs`): a second start brings the running launcher to front
  (restored if minimized) and quits - Windows via a named mutex + the window title, Linux via
  an abstract Unix socket. The launcher's own restarts (self-update, restart as admin) pass
  `--restarted`, so the new process waits for the old one instead of handing over to it -
  any new self-restart must pass it too. "Start game" refuses when one of the folder's
  `speed*.exe` is already running (process list; on Linux also the Wine process's .exe path).
- Texts: every user-visible string is a key in `launcher/lang/en.lang` (built in via
  `include_str!`); Slint uses `Tr.get(key, Tr.revision)` / `Tr.fmt(...)`, Rust `i18n::t` / `tf`.
  New text = add the key to **all** `lang/*.lang` files (`translations_complete` test fails
  otherwise). No `&` in texts (Windows menu mnemonics). `[Settings] Language`, else system
  language.
- Translations are **not shipped**: the launcher's Language menu downloads them from
  `launcher/lang/` on the default branch (GitHub contents API, listed live) into the game's
  `scripts/lang/`, so everything
  in that folder is published to users immediately after a push - only add verified files.
  Drafts (FR/ES/IT/RO/RU, written by Claude, unreviewed) are kept outside the repo until a
  native speaker has checked them.

## Releases

- Plugin: tag `v1.0.0.<build>`, asset `NFSU.Server.Changer.v<version>.zip` containing
  `dinput8.dll`, `readme.txt`, `scripts/{NFSUServerChanger.asi, .ini, NFSUServerChangerTrax.csv}`.
  The launcher release archive contains just the binary (no `lang/` folder).
  Bump the version in `plugin/NFSUServerChanger.rc` (FILEVERSION, PRODUCTVERSION and both
  strings).
- Launcher: separate release, assets must **not** start with `NFSU.Server.Changer.v`
  (the plugin updater would take them for plugin releases). The self-updater depends on the
  exact names: `NFSU.Server.Changer.Launcher.v<version>.Windows.zip` (containing
  `NFSUServerChanger.exe`) and `NFSU.Server.Changer.Launcher.v<version>.Linux.tar.gz`
  (containing `NFSUServerChanger`); the version is read from the asset name, the tag
  doesn't matter. Bump the Cargo version for every launcher release, or nobody gets it.
  Version in `launcher/Cargo.toml` as `1.0.0+<build>` (Cargo allows only 3 parts).

## Open / planned

- GitHub Actions workflow building both launcher versions for releases.
- Installer idea: choose the game folder, copy launcher and plugin there.
