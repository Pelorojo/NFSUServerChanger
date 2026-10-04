/*
█▄  █ █▀▀▀ ▄▀▀▀ █  █     ▄▀▀▀  ▄▄  ▄ ▄▄ ▄   ▄  ▄▄  ▄ ▄▄     ▄▀▀▀ █▄▄   ▄▄▄  ▄▄▄   ▄▄▄  ▄▄  ▄ ▄▄
█ ▀▄█ █▀▀   ▀▀▄ █  █      ▀▀▄ █▀▀▀ █▀   ▀▄ ▄▀ █▀▀▀ █▀       █    █  █ █  █  █  █ █  █ █▀▀▀ █▀
▀   ▀ ▀    ▀▀▀   ▀▀      ▀▀▀   ▀▀▀ ▀      ▀    ▀▀▀ ▀         ▀▀▀ ▀  ▀  ▀▀▀▀ ▀  ▀  ▀▀█  ▀▀▀ ▀
Hole Punch Rendevouz + Per-Server Logins + Wine/Proton Ping Fix + Custom EATrax   ▀▀
ASI Plugin for Need for Speed: Underground (2003) v1.4
Programmed by Redhair in 2026
*/
#include "stdafx.h"
#include "stdio.h"
#include <string>
#include <windows.h>
#include "includes\injector\injector.hpp"
#include "includes\IniReader.h"
#include "EATrax.h"
#include "PingFix.h"
#include "ServerLogins.h"
#include "HolePunch.h"

using namespace std;

const char* defaultSrv = "ps2nfs04.ea.com";

// The host name has a 32-byte slot in the .exe (all versions); right behind it lies the
// lobby port (10900) as a 32-bit value, which the game passes to its connect call.
// So at most 31 chars + terminating zero, or the port gets overwritten.
const size_t maxHostLen = 31;
const size_t portOffset = 32;
const DWORD defaultPort = 10900;

// "name:port" -> name and port, if it ends in ':' + a number 1-65535. Otherwise the whole
// value is the name and port stays 0.
void splitHostPort(const string& value, string& name, DWORD& port)
{
	name = value;
	port = 0;
	size_t colon = value.rfind(':');
	if (colon == string::npos || colon + 1 >= value.size() || value.size() - colon - 1 > 5)
		return;
	for (size_t i = colon + 1; i < value.size(); i++) {
		if (!isdigit(static_cast<unsigned char>(value[i])))
			return;
	}
	DWORD number = strtoul(value.c_str() + colon + 1, NULL, 10);
	if (number < 1 || number > 65535)
		return;
	name = value.substr(0, colon);
	port = number;
}

const uintptr_t serverAddrs[2] = { 0x6F1F40, 0x6F2078 }; // English, Russian
uintptr_t serverAddr;

const uintptr_t acceptAddrs[3] = { 0x734F54, 0x734F4C, 0x7354AC }; // North America, Europe, Russia
uintptr_t acceptAddr;

// EA Trax (EATrax.h): the game's song table.
const uintptr_t traxAddrs[2] = { 0x6F4D08, 0x6F4E40 }; // English, Russian
uintptr_t traxAddr;

// Ping fix (PingFix.h): the socket wrapper call in the pinger's constructor and the wrapper.
const uintptr_t pingCallAddrs[2] = { 0x66F1CB, 0x66EA3B }; // English, Russian
const uintptr_t socketWrapperAddrs[2] = { 0x662C30, 0x662490 }; // English, Russian
uintptr_t pingCallAddr;
uintptr_t socketWrapperAddr;

// The game resolves the lobby host name at two calls of its resolver. This one is in the
// login (reached from the account screens, starts a new online session); hooked to re-read
// the server from the ini there, so a server changed while the game runs (e.g. in the
// launcher) is used at the next login. The other call (0x54990B, from screens shown while
// logged in) is left alone, so a running session never gets mixed with another server.
const uintptr_t loginResolveCallAddrs[2] = { 0x54D534, 0x54D634 }; // English, Russian
const uintptr_t resolveHostAddrs[2] = { 0x668C50, 0x6684B0 }; // English, Russian
uintptr_t loginResolveCallAddr;
uintptr_t resolveHostAddr;

// Per-server logins (ServerLogins.h): the sprintf calls building the .pro file name
// in the reader and the writer, and the sprintf.
const uintptr_t proReadPathCallAddrs[2] = { 0x41D800, 0x41D8E0 }; // English, Russian
const uintptr_t proWritePathCallAddrs[2] = { 0x41D6E0, 0x41D7C0 }; // English, Russian
// ... and in the profile deletion
const uintptr_t proDeletePathCallAddrs[2] = { 0x41E095, 0x41E165 }; // English, Russian
const uintptr_t sprintfAddrs[2] = { 0x67101F, 0x67087F }; // English, Russian
// The push of the login's auth reply handler, the handler, and the game's login save.
const uintptr_t authReplyPushAddrs[2] = { 0x54D8F0, 0x54D9F0 }; // English, Russian
const uintptr_t authReplyAddrs[2] = { 0x54DC70, 0x54DD70 }; // English, Russian
const uintptr_t saveLoginAddrs[2] = { 0x549240, 0x549360 }; // English, Russian
// The login screen's call right before its "login loaded" check, and its target.
const uintptr_t preLoginCheckCallAddrs[2] = { 0x5578AE, 0x557A5E }; // English, Russian
const uintptr_t preLoginCheckAddrs[2] = { 0x558000, 0x558170 }; // English, Russian
uintptr_t proReadPathCallAddr;
uintptr_t proWritePathCallAddr;
uintptr_t proDeletePathCallAddr;
uintptr_t sprintfAddr;
uintptr_t authReplyPushAddr;
uintptr_t authReplyAddr;
uintptr_t saveLoginAddr;
uintptr_t preLoginCheckCallAddr;
uintptr_t preLoginCheckAddr2;

// The .exe's own server, for an empty Host (it may be patched, so it's read, not assumed).
char exeServer[maxHostLen + 1];

const char* iniFile = "NFSUServerChanger.ini";
const char* csvFile = "NFSUServerChangerTrax.csv";

// Directory of this .asi, with trailing backslash (the files next to it are found
// independently of the current directory the ASI loader happens to set).
string moduleDir()
{
	char path[MAX_PATH];
	HMODULE module = NULL;
	GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
		(LPCSTR)&moduleDir, &module);
	GetModuleFileNameA(module, path, sizeof(path));
	string dir = path;
	return dir.substr(0, dir.rfind('\\') + 1);
}

// Writes [Server] Host from the ini into the game. At start, and again at every login.
void ApplyServer()
{
	CIniReader iniReader(iniFile);

	// GameServer: "Host = name" or "Host = name:port"
	string hostName;
	DWORD hostPort;
	splitHostPort(iniReader.ReadString("Server", "Host", string(defaultSrv)), hostName, hostPort);

	// Written whenever set, ps2nfs04.ea.com included: the .exe may be patched to another
	// server, so the original one has to be set explicitly. Empty = keep the .exe's server.
	const char* server = hostName.empty() ? exeServer : hostName.c_str();
	char gameServerLtd[maxHostLen + 1];
	strncpy(gameServerLtd, server, maxHostLen);
	gameServerLtd[maxHostLen] = '\0';
	if (strcmp(reinterpret_cast<const char*>(serverAddr), gameServerLtd) != 0) {
		injector::WriteMemoryRaw(serverAddr, gameServerLtd, strlen(gameServerLtd) + 1, true);
	}

	// Port only with ":port" in Host; without one, only a non-standard port (patched .exe)
	// is set back to the standard one.
	DWORD port = hostPort ? hostPort : defaultPort;
	if (injector::ReadMemory<DWORD>(serverAddr + portOffset, true) != port) {
		injector::WriteMemory<DWORD>(serverAddr + portOffset, port, true);
	}
}

// The current server for its login file (ServerLogins.h): [Server] Host (with :port, if given)
// or the .exe's server, lower case, other chars than letters, digits, '.' and '-' as '_'.
string CurrentServer()
{
	CIniReader iniReader(iniFile);
	string server = iniReader.ReadString("Server", "Host", string(defaultSrv));
	while (!server.empty() && server.back() == ' ')
		server.pop_back();
	if (server.empty())
		server = exeServer;
	for (size_t i = 0; i < server.size(); i++) {
		unsigned char c = static_cast<unsigned char>(server[i]);
		server[i] = (isalnum(c) || c == '.' || c == '-') ? static_cast<char>(tolower(c)) : '_';
	}
	return server;
}

// The game's resolver: looks up the host name, with a timeout in ms.
typedef void* (__cdecl *ResolveHost)(const char* host, int timeoutMs);

void* __cdecl ResolveServer(const char* host, int timeoutMs)
{
	// Server changed while the login screen was open: the shown login is another server's, so
	// the login fails like an unreachable server (ServerLogins.h).
	if (LoginServerChanged())
		return NULL;
	ApplyServer();
	return reinterpret_cast<ResolveHost>(resolveHostAddr)(host, timeoutMs);
}

int Init()
{
	CIniReader iniReader(iniFile);

	// EATrax
	ApplyTrax(traxAddr, moduleDir() + csvFile, iniReader.ReadString("EATrax", "NoQuotes", string("0")) != "1");

	strncpy(exeServer, reinterpret_cast<const char*>(serverAddr), maxHostLen);
	exeServer[maxHostLen] = '\0';
	ApplyServer();

	// Re-read at every login; only if the .exe has exactly this call there.
	if (injector::ReadMemory<BYTE>(loginResolveCallAddr, true) == 0xE8
		&& injector::GetBranchDestination(loginResolveCallAddr, true).as_int() == resolveHostAddr) {
		injector::MakeCALL(loginResolveCallAddr, &ResolveServer, true);
	}

	typedef void* memory_pointer_tr;
	memory_pointer_tr memAddr = reinterpret_cast<memory_pointer_tr>(acceptAddr);
	injector::MemoryFill(memAddr, 1, 1, true);

	InstallPingFix(pingCallAddr, socketWrapperAddr);

	// [HolePunch] Log = 1: NFSUServerChanger_HolePunch.log next to the .asi.
	HolePunch::Install(iniReader.ReadString("HolePunch", "Log", string("0")) == "1");

	InstallServerLogins(proReadPathCallAddr, proWritePathCallAddr, proDeletePathCallAddr, sprintfAddr,
		authReplyPushAddr, authReplyAddr, saveLoginAddr,
		preLoginCheckCallAddr, preLoginCheckAddr2);

	return 0;
}

// Function to get the checksum
DWORD GetCheckSum(IMAGE_NT_HEADERS* nt)
{
	return nt->OptionalHeader.CheckSum; // Return the checksum from the optional header
}

BOOL APIENTRY DllMain(HMODULE /*hModule*/, DWORD reason, LPVOID /*lpReserved*/)
{
	if (reason == DLL_PROCESS_ATTACH)
	{
		uintptr_t base = (uintptr_t)GetModuleHandleA(NULL);
		IMAGE_DOS_HEADER* dos = (IMAGE_DOS_HEADER*)(base);
		IMAGE_NT_HEADERS* nt = (IMAGE_NT_HEADERS*)(base + dos->e_lfanew);

		uintptr_t entryPoint = base + nt->OptionalHeader.AddressOfEntryPoint + (0x400000 - base);

		// Check if the executable is compatible
		if (entryPoint == 0x670CB5 || entryPoint == 0x670515) // English or SoftClub
		{
			// Get the current checksum
			DWORD currentChecksum = GetCheckSum(nt);

			// Check against the expected checksums
			int v, a; // v = address set index, a = acceptAddrs index
			switch (currentChecksum)
			{
			case 0x003126F3: v = 0; a = 0; break; // North America
			case 0x00314E20: v = 0; a = 1; break; // Europe
			case 0x0030CBA0: v = 1; a = 2; break; // Russia
			default:
				MessageBoxA(NULL, "This .exe version is not supported.\nPlease use the correct version.", "NFSU Server Changer", MB_ICONERROR);
				return FALSE;
			}

			serverAddr = serverAddrs[v];
			traxAddr = traxAddrs[v];
			acceptAddr = acceptAddrs[a];
			loginResolveCallAddr = loginResolveCallAddrs[v];
			resolveHostAddr = resolveHostAddrs[v];
			pingCallAddr = pingCallAddrs[v];
			socketWrapperAddr = socketWrapperAddrs[v];
			proReadPathCallAddr = proReadPathCallAddrs[v];
			proWritePathCallAddr = proWritePathCallAddrs[v];
			proDeletePathCallAddr = proDeletePathCallAddrs[v];
			sprintfAddr = sprintfAddrs[v];
			authReplyPushAddr = authReplyPushAddrs[v];
			authReplyAddr = authReplyAddrs[v];
			saveLoginAddr = saveLoginAddrs[v];
			preLoginCheckCallAddr = preLoginCheckCallAddrs[v];
			preLoginCheckAddr2 = preLoginCheckAddrs[v];
			Init();
		}
		else
		{
			MessageBoxA(NULL, "This .exe is not supported.\nPlease use v1.4 of Speed.exe (3,03 MB (3.178.496 bytes)).", "NFSU Server Changer", MB_ICONERROR);
			return FALSE;
		}
	}
	return TRUE;
}
