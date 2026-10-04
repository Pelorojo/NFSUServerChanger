# NFSUServerChanger launcher

Launcher for the NFSUServerChanger plugin (`../plugin`): pick a Need for Speed:
Underground online server, write it to `scripts/NFSUServerChanger.ini`, start the game.

Written in Rust with [Slint](https://slint.dev). Native builds for Windows and Linux.

## Usage

Copy `NFSUServerChanger.exe` (Linux: `NFSUServerChanger`) into the game folder (the one containing `scripts/`) and run it.
The plugin (`scripts/NFSUServerChanger.asi` + `.ini`, loaded by an ASI loader) must be installed too -
without it the game never reads the ini. The launcher shows a red warning at the top if it's missing,
with an **Install plugin** button, and asks before starting the game.

**Plugin install / updates:** the launcher downloads the newest plugin release
(`NFSU.Server.Changer.v<version>.zip`) from GitHub and extracts it into the game folder -
skipping `readme.txt` and `NFSUServerChangerTrax.csv` (the launcher creates the track list from
its built-in template the first time you edit it), keeping an existing `dinput8.dll`, and keeping an existing
`NFSUServerChanger.ini` (only settings/sections it doesn't have yet are added; the list
sections `[FixedServers]`, `[PublicList]`, `[Community]` only when missing entirely).
The installed version is read from the .asi's version info. At every start (and via
**Check for updates** in the menu) the launcher looks for newer releases and, if there are
any, shows them in a popup; nothing is installed without clicking **Update**. After a plugin
update the launcher re-reads everything right away, no restart needed.
Click a server and it is set right away (or type any hostname/IP and press Enter;
**Start game** also sets whatever is in the field). An empty host keeps the server built into the game's .exe
(originally `ps2nfs04.ea.com`, unless the .exe is patched); the menu can also set the original
EA server explicitly. The `Host` line in the `[Server]` section is the
only thing changed; everything else in the ini stays as is.

The list is built locally, no web API involved: the fixed public servers from
`[FixedServers]` in `NFSUServerChanger.ini` (`Name = hostname or IP`),
the public lists of the trackers (`http://<provider>/tracker/get_list.php`, one `Name = host or URL`
line each in `[PublicList]` of `NFSUServerChanger.ini`, default nfsug.harpywar.com and nfs.onl), plus everything in
`servers.cache` (written by the app) and `servers.dat`. Every server is queried directly
on its lobby status port (10980); a server without one is checked for an EA Nation server
via its `@tic` echo on port 5000, like kickStart does.

Offline servers are hidden, except the ones in your own list: `servers.dat` in the
game folder, one IP or hostname per line. Use **Add** / **Remove** to edit it from the app.

**Start game** runs the selected `speed*.exe` from the game folder (Linux: via `wine` -
the system one, else the newest wine runner of Lutris - with `WINEPREFIX` taken from a
`.../drive_c/...` path). A custom command can be set
as `Command` in `[Launch]` of `NFSUServerChanger.ini`: `{exe}` is replaced by the exe path,
without it the path is appended (so `gamemoderun wine` works as a prefix). The selected exe
is remembered there too (`Game`).
The plugin ignores these sections.

The menu (☰ next to Refresh) has File (open the game folder, edit `NFSUServerChanger.ini`,
`NFSUServerChangerTrax.csv` and `servers.dat`), Server, Community, **Settings** and About.
Settings (stored in `[Settings]` of the ini): the **editor** for the Edit entries - a command,
`{file}` = the file (appended if missing), preset `notepad`, empty = system default - and which
**public server lists** from `[PublicList]` are queried (preset: NFS.onl only).

**Pings (Linux):** the game pings servers over a raw ICMP socket, which Wine (7.13 or newer) can
only use when `net.ipv4.ping_group_range` allows the user's groups - many distros allow nobody.
Without it the plugin keeps the game from crashing, but its pings time out. At start the
launcher checks this (and the Wine version the game starts with) and offers to switch pings
on: **Switch on now** runs the command via `pkexec` (the system's password dialog), or copy it
and run it yourself. **Don't ask again** sets `[Settings] PingHint = 0`; the settings dialog
shows the state and can switch it on any time.

The **Community** menu lists the links from `[Community]` in `NFSUServerChanger.ini`
(`Name = URL`, in file order) - edit, remove or add your own there. All these lists come only
from the ini; the one shipped with the plugin has them preset.

**Launcher updates:** the launcher updates itself the same way, from its own release assets
(`NFSU.Server.Changer.Launcher.v<version>.Windows.zip` / `.Linux.tar.gz`), offered in the
same popup. After **Update** it restarts into the new version by itself; the status line
shows each step.

**Languages:** English is built in; nothing else ships with the launcher. **Language ->
Download languages…** lists the `.lang` files in `launcher/lang/` of this repo (default
branch, read live via the GitHub API) and installs, updates or removes them in the game's
`scripts/lang` folder; installing switches to it right away. A file pushed there shows up for
everyone without a launcher release; a changed one is offered as an update. The choice is
stored as `[Settings] Language`; without it the system language is used if it's installed.
`lang/en.lang` is the template for new translations - copy it, set `Language` to the
language's own name and translate the texts; missing lines stay English. `cargo test` checks
that every file in `lang/` has all keys and placeholders.

## Building

Needs Rust: install it via <https://rustup.rs> (Windows: run `rustup-init.exe`, keep the defaults).
Then, in this folder:

```sh
cargo build --release   # -> target/release/NFSUServerChanger(.exe)
cargo test              # run the tests
cargo run               # build and start (needs a game folder with scripts/NFSUServerChanger.ini)
```

The first build takes a few minutes (Slint gets compiled), later ones are much faster.

### Linux

Needs the usual desktop dev packages, e.g. on Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libfontconfig-dev libxkbcommon-dev libwayland-dev
```

### Windows

Rust uses Microsoft's linker (`link.exe`) from Visual Studio to put the `.exe` together, so
Visual Studio (or the Build Tools) with the "Desktop development with C++" workload is needed -
already there if you build the plugin. Nothing else to set up, Rust finds it by itself.

1. Download and run `rustup-init.exe` from <https://rustup.rs>, press Enter for the standard
   installation. It detects Visual Studio (and warns if the C++ workload is missing).
2. Open a **new** terminal (PowerShell or cmd, no Developer Command Prompt needed), so
   `cargo` is on the PATH.
3. Build:
   ```powershell
   cd <repo>\launcher
   cargo build --release
   ```
   The result is `target\release\NFSUServerChanger.exe`.

`rustup show` should list `stable-x86_64-pc-windows-msvc` as the toolchain (`msvc` = using the
Visual Studio linker).

### Windows .exe from Linux/WSL

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/NFSUServerChanger.exe
```

Building in WSL directly on `/mnt/c/...` is slow; putting the build output on the Linux side
helps, e.g. `export CARGO_TARGET_DIR=~/cargo-target/nfsu-launcher`.

## Editing the UI

The UI lives in `ui/app.slint`. With the Slint extension for VS Code
(see `.vscode/extensions.json`) you get a live preview and a property editor:
open the file and click "Show Preview" above `AppWindow`.
