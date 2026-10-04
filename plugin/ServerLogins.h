// Separate logins per server: every server has its own accounts, but the game keeps only one
// account per profile, "<profile>0.pro" in its save folder. The game builds that file name
// with sprintf("%s%s%d%s", folder, profile, 0, ".pro") when it reads it (online login screen)
// and when it writes it (after a login with saved password); both calls are redirected to
// "<profile>0_<hash>.pro" in the same folder, <hash> = FNV-1a (32 bit, hex) of the server
// ([Server] Host, see CurrentServer). So every server has its own file and nothing is ever
// copied. Works for any launcher, and for a server changed while the game runs.
//
// Reading falls back to the plain "<profile>0.pro" while a server has no file of its own yet
// (the login from before, or of the original game); the login screen then shows that one.
// The game itself only writes the file when name or password were edited in the login
// screen, so after a successful login with such a fallback the game's save function is
// called once: it writes exactly the login that just worked into the server's own file.
//
// The login screen reads the file only while the game has no login loaded yet (a flag the
// game sets after loading it); after a server change while the game runs, that flag is
// cleared right before the screen checks it, so the new server's file gets read.
//
// Not redirected: deleting a profile in the game still deletes only "<profile>0.pro"; its
// per-server files stay in the save folder.
#pragma once

#include <windows.h>
#include <string>
#include "includes\injector\injector.hpp"

// The current server, normalized, and writing [Server] Host into the game (dllmain.cpp).
std::string CurrentServer();
void ApplyServer();

// The game's sprintf.
typedef int (__cdecl *GameSprintf)(char* buffer, const char* format, ...);
static GameSprintf gameSprintf;

// The game's login save (copies the current login into its buffer and calls the writer) and
// the reply handler of the login's auth request.
typedef void (__cdecl *GameSaveLogin)();
typedef void (__cdecl *GameAuthReply)(int context, int* reply);
static GameSaveLogin gameSaveLogin;
static GameAuthReply gameAuthReply;

// The game's "login loaded" flag (byte) and the call in the login screen right before it is
// checked, with the function it calls (it takes arguments in registers).
static uintptr_t loginLoadedFlagAddr;
static uintptr_t preLoginCheckAddr;

// The server the login was last read for (empty: not read yet).
static std::string proReadServer;

// Set by the reader when it fell back to the plain file: the server and its own file then.
static bool proFallback;
static std::string proFallbackServer;
static std::string proFallbackPath;

// The game's buffer for the path (264 bytes in both functions).
static const size_t proPathSize = 264;

static DWORD Fnv1a(const std::string& text)
{
	DWORD hash = 0x811C9DC5;
	for (size_t i = 0; i < text.size(); i++) {
		hash ^= static_cast<unsigned char>(text[i]);
		hash *= 0x01000193;
	}
	return hash;
}

// The per-server name of the .pro file into buffer; false without a server or for too long
// a path.
static bool ServerProPath(char* buffer, const char* folder, const char* profile, int number, const char* ext, std::string& server)
{
	try {
		server = CurrentServer();
	}
	catch (...) {
	}
	if (server.empty())
		return false;
	char path[proPathSize];
	int length = _snprintf(path, sizeof(path), "%s%s%d_%08x%s", folder, profile, number, Fnv1a(server), ext);
	if (length <= 0 || static_cast<size_t>(length) >= sizeof(path))
		return false;
	memcpy(buffer, path, length + 1);
	return true;
}

// Reader: the server's file if it exists, otherwise the game's own name.
static int __cdecl ProPathRead(char* buffer, const char* format, const char* folder, const char* profile, int number, const char* ext)
{
	std::string server;
	proFallback = false;
	if (ServerProPath(buffer, folder, profile, number, ext, server)) {
		proReadServer = server;
		if (GetFileAttributesA(buffer) != INVALID_FILE_ATTRIBUTES)
			return static_cast<int>(strlen(buffer));
		proFallback = true;
		proFallbackServer = server;
		proFallbackPath = buffer;
	}
	return gameSprintf(buffer, format, folder, profile, number, ext);
}

// Writer: always the server's file (the game's own name only without a server).
static int __cdecl ProPathWrite(char* buffer, const char* format, const char* folder, const char* profile, int number, const char* ext)
{
	std::string server;
	if (ServerProPath(buffer, folder, profile, number, ext, server))
		return static_cast<int>(strlen(buffer));
	return gameSprintf(buffer, format, folder, profile, number, ext);
}

// Auth reply of the login: after a successful login (status 0) with the fallback file, still
// on the same server and without its own file yet, the game saves that login - to the
// server's own file (ProPathWrite).
static void __cdecl AuthReply(int context, int* reply)
{
	gameAuthReply(context, reply);
	if (!proFallback || reply == NULL || reply[2] != 0)
		return;
	proFallback = false;
	try {
		if (CurrentServer() == proFallbackServer && GetFileAttributesA(proFallbackPath.c_str()) == INVALID_FILE_ATTRIBUTES)
			gameSaveLogin();
	}
	catch (...) {
	}
}

// Login screen, right before the "login loaded" check: the server from the ini is written into
// the game already here (not only at the connect), so everything shown on the login screen
// (e.g. a server name other plugins display) is the current one; and a login loaded for
// another server is dropped, so the screen reads the current server's file.
static void CheckLoginServer()
{
	try {
		ApplyServer();
		if (proReadServer.empty() || injector::ReadMemory<BYTE>(loginLoadedFlagAddr, true) == 0)
			return;
		if (CurrentServer() != proReadServer)
			injector::WriteMemory<BYTE>(loginLoadedFlagAddr, 0, true);
	}
	catch (...) {
	}
}

static void __declspec(naked) PreLoginCheckHook()
{
	__asm {
		pushad
		pushfd
		call CheckLoginServer
		popfd
		popad
		jmp [preLoginCheckAddr]
	}
}

// The sprintf calls in the .pro reader and writer, the sprintf they call; the push of the auth
// reply handler in the login, the handler, and the game's login save. Patched only if the .exe
// has exactly these instructions there; the save after a fallback login only if the push
// matches too; the re-read after a server change only if the call before the flag check
// matches and is followed by "mov al, [flag]" - the flag's address is taken from there (it
// differs between the North American and the European .exe).
inline void InstallServerLogins(uintptr_t readCallAddr, uintptr_t writeCallAddr, uintptr_t sprintfAddr,
	uintptr_t authReplyPushAddr, uintptr_t authReplyAddr, uintptr_t saveLoginAddr,
	uintptr_t preCheckCallAddr, uintptr_t preCheckAddr)
{
	const uintptr_t calls[2] = { readCallAddr, writeCallAddr };
	for (int i = 0; i < 2; i++) {
		if (injector::ReadMemory<BYTE>(calls[i], true) != 0xE8)
			return;
		if (injector::GetBranchDestination(calls[i], true).as_int() != sprintfAddr)
			return;
	}
	gameSprintf = reinterpret_cast<GameSprintf>(sprintfAddr);
	injector::MakeCALL(readCallAddr, &ProPathRead, true);
	injector::MakeCALL(writeCallAddr, &ProPathWrite, true);

	if (injector::ReadMemory<BYTE>(authReplyPushAddr, true) == 0x68
		&& injector::ReadMemory<uintptr_t>(authReplyPushAddr + 1, true) == authReplyAddr) {
		gameAuthReply = reinterpret_cast<GameAuthReply>(authReplyAddr);
		gameSaveLogin = reinterpret_cast<GameSaveLogin>(saveLoginAddr);
		injector::WriteMemory<uintptr_t>(authReplyPushAddr + 1, reinterpret_cast<uintptr_t>(&AuthReply), true);
	}

	if (injector::ReadMemory<BYTE>(preCheckCallAddr, true) == 0xE8
		&& injector::GetBranchDestination(preCheckCallAddr, true).as_int() == preCheckAddr
		&& injector::ReadMemory<BYTE>(preCheckCallAddr + 5, true) == 0xA0) {
		preLoginCheckAddr = preCheckAddr;
		loginLoadedFlagAddr = injector::ReadMemory<uintptr_t>(preCheckCallAddr + 6, true);
		injector::MakeCALL(preCheckCallAddr, &PreLoginCheckHook, true);
	}
}
