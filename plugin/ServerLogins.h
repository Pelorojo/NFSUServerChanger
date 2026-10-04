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
// game sets after loading it); after a change of the server or the profile since that file
// was read, the flag is cleared right before the screen checks it, so the current server's
// file of the current profile gets read.
//
// Deleting a profile in the game deletes its per-server files too (the game itself deletes
// "<profile>0.pro").
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

// The game's current profile name (taken from the reader's sprintf arguments, it differs
// between the North American and the European .exe); 0 = unknown.
static uintptr_t profileNameAddr;

// The server and the profile the login was last read for (empty: not read yet).
static std::string proReadServer;
static std::string proReadProfile;

static std::string CurrentProfile()
{
	if (profileNameAddr == 0)
		return std::string();
	const char* name = reinterpret_cast<const char*>(profileNameAddr);
	return std::string(name, strnlen(name, 64));
}

// The game's active login (account name first; taken from the login save, which copies it).
// The save after a fallback login only happens when it holds a name: after "use another
// account" the typed login is still elsewhere at that point, and an empty file would hide
// the profile's login from then on. 0 = unknown, no save.
static uintptr_t activeLoginAddr;

// Whether a .pro file holds a login (an account name at offset 8). One without is treated
// as missing, so it can't hide the plain file.
static bool HasLogin(const char* path)
{
	FILE* f = fopen(path, "rb");
	if (f == NULL)
		return false;
	unsigned char head[9];
	size_t got = fread(head, 1, sizeof(head), f);
	fclose(f);
	return got == sizeof(head) && head[8] != 0;
}

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
		proReadProfile = profile;
		if (HasLogin(buffer))
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

// Profile deletion: the game's own name (it deletes that file itself), and the profile's
// per-server files "<profile><number>_<8 hex digits>.pro" are deleted here.
static int __cdecl ProPathDelete(char* buffer, const char* format, const char* folder, const char* profile, int number, const char* ext)
{
	int result = gameSprintf(buffer, format, folder, profile, number, ext);
	try {
		char prefix[proPathSize];
		int prefixLength = _snprintf(prefix, sizeof(prefix), "%s%d_", profile, number);
		if (prefixLength > 0 && static_cast<size_t>(prefixLength) < sizeof(prefix)) {
			std::string dir = folder;
			WIN32_FIND_DATAA data;
			HANDLE find = FindFirstFileA((dir + prefix + "*" + ext).c_str(), &data);
			if (find != INVALID_HANDLE_VALUE) {
				const size_t extLength = strlen(ext);
				do {
					// exactly prefix + 8 hex digits + ext, so no other profile's file can match
					const char* name = data.cFileName;
					if (strlen(name) != prefixLength + 8 + extLength || _strnicmp(name, prefix, prefixLength) != 0
						|| _stricmp(name + prefixLength + 8, ext) != 0)
						continue;
					bool hex = true;
					for (int i = 0; i < 8; i++)
						hex = hex && isxdigit(static_cast<unsigned char>(name[prefixLength + i])) != 0;
					if (hex)
						DeleteFileA((dir + name).c_str());
				} while (FindNextFileA(find, &data));
				FindClose(find);
			}
		}
	}
	catch (...) {
	}
	return result;
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
		if (CurrentServer() == proFallbackServer && !HasLogin(proFallbackPath.c_str())
			&& activeLoginAddr != 0 && injector::ReadMemory<char>(activeLoginAddr, true) != 0)
			gameSaveLogin();
	}
	catch (...) {
	}
}

// Login screen, right before the "login loaded" check: the server from the ini is written into
// the game already here (not only at the connect), so everything shown on the login screen
// (e.g. a server name other plugins display) is the current one; and a login loaded for
// another server or another profile is dropped, so the screen reads the right file.
static void CheckLoginServer()
{
	try {
		ApplyServer();
		if (proReadServer.empty() || injector::ReadMemory<BYTE>(loginLoadedFlagAddr, true) == 0)
			return;
		if (CurrentServer() != proReadServer || (profileNameAddr != 0 && CurrentProfile() != proReadProfile))
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

// At the connect of a login: true if the server was changed after the login screen loaded its
// login (the shown login belongs to another server). The login is then not sent anywhere; the
// next time the login screen opens, it loads the current server's login.
static bool LoginServerChanged()
{
	try {
		return !proReadServer.empty() && CurrentServer() != proReadServer;
	}
	catch (...) {
		return false;
	}
}

// The sprintf calls in the .pro reader and writer, the sprintf they call; the push of the auth
// reply handler in the login, the handler, and the game's login save. Patched only if the .exe
// has exactly these instructions there; the save after a fallback login only if the push
// matches too; the re-read after a server change only if the call before the flag check
// matches and is followed by "mov al, [flag]" - the flag's address is taken from there (it
// differs between the North American and the European .exe).
inline void InstallServerLogins(uintptr_t readCallAddr, uintptr_t writeCallAddr, uintptr_t deleteCallAddr, uintptr_t sprintfAddr,
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
	// "push <profile name>" 0x17 bytes before the reader's sprintf call
	if (injector::ReadMemory<BYTE>(readCallAddr - 0x17, true) == 0x68)
		profileNameAddr = injector::ReadMemory<uintptr_t>(readCallAddr - 0x17 + 1, true);
	injector::MakeCALL(readCallAddr, &ProPathRead, true);
	injector::MakeCALL(writeCallAddr, &ProPathWrite, true);

	if (injector::ReadMemory<BYTE>(deleteCallAddr, true) == 0xE8
		&& injector::GetBranchDestination(deleteCallAddr, true).as_int() == sprintfAddr)
		injector::MakeCALL(deleteCallAddr, &ProPathDelete, true);

	if (injector::ReadMemory<BYTE>(authReplyPushAddr, true) == 0x68
		&& injector::ReadMemory<uintptr_t>(authReplyPushAddr + 1, true) == authReplyAddr) {
		gameAuthReply = reinterpret_cast<GameAuthReply>(authReplyAddr);
		gameSaveLogin = reinterpret_cast<GameSaveLogin>(saveLoginAddr);
		// "mov eax, [active login]" 0x15 bytes into the save
		if (injector::ReadMemory<BYTE>(saveLoginAddr + 0x15, true) == 0xA1)
			activeLoginAddr = injector::ReadMemory<uintptr_t>(saveLoginAddr + 0x16, true);
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
