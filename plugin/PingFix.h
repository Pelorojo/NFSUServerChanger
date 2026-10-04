// Ping fix: the game pings servers over its own raw ICMP socket. Linux only allows that with
// extra rights; when Wine can't create the socket, the game's ping manager is NULL and the
// game crashes in its ping code (read access to 0000000C at 00668791).
//
// The one socket(AF_INET, SOCK_RAW, IPPROTO_ICMP) call is redirected: if the raw socket
// fails, a plain UDP socket is created instead. The ping manager then gets built normally,
// its echo requests go nowhere (the game already handles failing sends) and pings time out
// instead of crashing. Where the raw socket works (Windows, or Linux with
// net.ipv4.ping_group_range on Wine 7.13+), nothing changes.
#pragma once

#include <winsock2.h>
#include "includes\injector\injector.hpp"

// The game's socket wrapper: socket() + allocation of its socket object, NULL on failure.
typedef void* (__cdecl *GameSocket)(int af, int type, int protocol);
static GameSocket gameSocket;

static void* __cdecl PingSocket(int af, int type, int protocol)
{
	void* sock = gameSocket(af, type, protocol);
	if (sock == NULL) {
		sock = gameSocket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	}
	return sock;
}

// callAddr: the call of the socket wrapper in the pinger's constructor, wrapperAddr: the
// wrapper it calls. Patched only if the .exe has exactly that call there.
inline void InstallPingFix(uintptr_t callAddr, uintptr_t wrapperAddr)
{
	if (injector::ReadMemory<BYTE>(callAddr, true) != 0xE8)
		return;
	if (injector::GetBranchDestination(callAddr, true).as_int() != wrapperAddr)
		return;
	gameSocket = reinterpret_cast<GameSocket>(wrapperAddr);
	injector::MakeCALL(callAddr, &PingSocket, true);
}
