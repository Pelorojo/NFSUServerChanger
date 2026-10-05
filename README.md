# NFSU Server Changer
NFSU Server Changer is a script mod which can change the gameserver without HEX editing the .exe file manually,
but also features renaming EATrax, see .ini file for details.

There are following options:

Game server:
+ Host (-name or IP)

The server is read again at every login, so it can be changed while the game runs (in the ini
or the launcher): log out, go back to the account screen and log in again. A running online
session keeps its server.

Per-Server Logins (always on):

Every server has its own accounts, but the game keeps only one saved login per profile
(`<profile>0.pro` in its save folder). Here, each server gets its own file,
`<profile>0_<hash>.pro` (the hash is the same for the same Host), so switching servers no longer
means typing the login again. While a server has no file yet, the login from `<profile>0.pro` is
shown; after the first successful login there, it is saved for that server. Changing the server
while the game runs works too: the login screen loads the current server's login when it opens.
If the server is changed while the login screen is already open, that login fails (like an
unreachable server) instead of sending the other server's login - just open the screen again.

Hole Punching (always on):
+ Log (1 = write NFSUServerChanger_HolePunch.log next to the .asi; default 0)

Races are peer-to-peer: every player talks to the host over UDP 3658, but the lobby only tells
the players each other's IP, never a port. Behind a router without port forwarding or on mobile
internet (carrier-grade NAT), the game's guess of port 3658 is wrong and the race never starts.
On servers that run the rendezvous service (UDP 10910 on the lobby server, part of nfsuinfoserver),
the plugin tells the server its real public port from the game's own socket and asks it for the
other players' ports; the game's packets then go to the right port, and the game's own start
packets open both NATs. If nothing arrives directly after a few seconds, the race is relayed
through the server instead. Players without the plugin, and servers without the service, are
not affected. A local firewall (e.g. ufw on Linux) still has to allow UDP 3658-3659.

Trax Renamer:
+ NoQuotes

Linux fix (always on, no option): under Wine the game crashes when it can't open its ping socket
("Failed to create a socket of type SOCK_RAW", then a page fault at 00668791). The plugin prevents
that; the game's pings then time out. For working pings (Wine 7.13 or newer), allow ping sockets
once - the launcher offers to do this, or run it yourself:

    sudo sh -c "echo 'net.ipv4.ping_group_range = 0 2147483647' > /etc/sysctl.d/99-nfsu-ping.conf && sysctl -p /etc/sysctl.d/99-nfsu-ping.conf"

# Repository layout
+ `plugin/` - the .asi script mod (C++, Visual Studio: `plugin/NFSUServerChanger.sln`)
+ `launcher/` - optional launcher `NFSUServerChanger.exe` (Rust + Slint, Windows and Linux): pick a server
  from a live list and start the game. See [launcher/README.md](launcher/README.md).

# Building
+ **Plugin:** open `plugin/NFSUServerChanger.sln` in Visual Studio (with "Desktop development with C++")
  and build; the output is `NFSUServerChanger.asi`, which goes into the game's `scripts` folder together
  with `NFSUServerChanger.ini` and `NFSUServerChangerTrax.csv`.
+ **Launcher:** see [launcher/README.md](launcher/README.md#building) (Rust: `cargo build --release`
  in `launcher/`, for Windows and Linux).

# Download
You can [download Server Changer](https://github.com/Pelorojo/NFSUServerChanger/releases) from Releases page, or from [NFS.onl](https://nfs.onl/files/asi).
If you want to compile it yourself, you can download the source code from the green Clone or Download button up there.

# Guide and help
+ [Wiki](https://github.com/Pelorojo/NFSUServerChanger/wiki): installation, launcher, per-server logins,
  hole punching, Linux, troubleshooting
+ Questions: `#nfs-underground` on the [Retro Racing Point Discord](https://discord.gg/JMaypEd)

# Credits
+ Programmed by Redhair
+ Originally forked from [NFSU Trax Renamer](https://github.com/nlgxzef/NFSUTraxRenamer) by nlgzrgn, where the EA Trax
  renaming started
+ [Injectors](https://github.com/thelink2012/injector) by LINK/2012 and the ini parser by Denilson das Mercês Amorim and
  ThirteenAG (`plugin/includes/`)
+ Launcher UI made with [Slint](https://slint.dev)
