NFSU Server Changer
v1.0.0.1994

Source Code: https://github.com/Pelorojo/NFSUServerChanger
Guide:       https://github.com/Pelorojo/NFSUServerChanger/wiki
Help:        #nfs-underground on the Retro Racing Point Discord, https://discord.gg/JMaypEd
------------------------------------------------------------------------------------------------------------
Description:

NFSU Server Changer is a script mod which can change the gameserver without HEX editing the .exe file
manually. It also keeps a separate saved login per server, makes races work behind routers without port
forwarding (hole punching), keeps the game from crashing under Wine and features a custom EA Games Trax
playlist. Check the .ini file for details. There are following options:

Server:
+ Host (-name or IP)

EATrax:
+ NoQuotes (for titles without quotes)

HolePunch:
+ Log (1 = write NFSUServerChanger_HolePunch.log, to find connection problems)

Optional: the launcher NFSUServerChanger.exe (Windows and Linux) lists the servers, sets the one you
click and starts the game. Download it from the Releases page.
------------------------------------------------------------------------------------------------------------
Installation:

! Before installation, make sure that you're using v1.4 speed.exe (3,03 MB (3.178.496 bytes)).
If not, just search "NFS Underground v1.4 NO CD Crack DRUNK!" on Google, Bing or any search engine you like.

Now you can install the Script. Just follow these steps:

1) Open the archive you downloaded
2) Extract "scripts" folder and "dinput8.dll" into your NFSU Installation Folder.
------------------------------------------------------------------------------------------------------------
Changelog: (+ Addition, * Change, ! Attention, - Deletion)

v1.0.0.1994 (Build 1; Rev.00) :
+ Per-server logins: every server keeps its own saved login (<profile>0_<hash>.pro)
+ Hole punching: races work without port forwarding and on mobile internet, on servers that support it
+ Linux/Wine: no crash when the game can't open its ping socket
* Server is read again at every login, so it can be changed while the game runs

v1.0.0.1993 (Build 1; Rev.00) :
* Memory address of agree switch fixed for EU version

v1.0.0.1992 (Build 1; Rev.00) :
+ Compatible with NA, EU and RU (SoftClub) version

v1.0.0.1987 (Build 1; Rev.00) :
+ Songs can be disabled with "x"

v1.0.0.1986 (Build 1; Rev.00) :
+ Caret (^) can be used as placeholder for semicolon (;)

v1.0.0.1985 (Build 1; Rev.00) :
+ EATrax playlist from .csv file
+ Online account agreement now Accept as default

v1.0.0.1337 (Build 1; Rev.00) :
+ Initial release.
------------------------------------------------------------------------------------------------------------
Credits:

Programmed by Redhair, forked from "NFSU Trax Renamer" by nlgzrgn
