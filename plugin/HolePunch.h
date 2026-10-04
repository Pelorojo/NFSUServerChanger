// Hole punching (HolePunch.h) - UDP hole punching for NFS Underground races.
//
// The race is a star: every client talks to the host over UDP 3658. The lobby only tells
// the players each other's public IP (`ADDRn` in `+ses`), never a port, so the game always
// sends to ADDRn:3658. That fails whenever the host's NAT doesn't keep 3658 (mobile/CGNAT,
// no port forwarding). This plugin learns the real public port of every player and sends
// there instead:
//
// 1. HELLO: from the game's own UDP socket, `NHP1 HELLO <persona>` to the rendezvous service
//    on the lobby server (UDP RENDEZVOUS_PORT). The service sees our public ip:port - the
//    same NAT mapping the race will use. Repeated every KEEPALIVE_MS so the mapping stays.
// 2. The lobby connection (TCP) is read along: `pers` gives our persona, `+ses` (race start)
//    the other players' personas and public IPs.
// 3. QUERY: from a socket of our own (answers never reach the game's socket), the service is
//    asked for each player's port. Only the port is taken over, and only for exactly the IP
//    the lobby reported in ADDRn - nobody can redirect traffic to a foreign IP.
// 4. sendto() to ADDRn:3658 goes to ADDRn:<real port>; recvfrom() reports packets from
//    ADDRn:<real port> to the game as ADDRn:3658. The game's own start packets (host `05`,
//    client `01`, sent at the same time) then open both NATs - no extra punch packets needed.
//
// 5. Relay fallback: if nothing arrives from a peer RELAY_AFTER_MS after the race start (both
//    NATs symmetric, e.g. mobile or WARP), the game's packets for that peer go to the service
//    instead, wrapped as `NHPR <4-byte pair token> <packet>`, and the service forwards them to
//    the peer's plugin, which unwraps them and reports them to the game as ADDRn:3658. Relayed
//    packets arriving from a peer switch us to the relay for it too, so both sides follow
//    even if only one direction failed. Only for peers that have the plugin (found by QUERY,
//    possibly under another IP: WARP uses different IPs for TCP and UDP - fine for the relay,
//    where both sides only talk to the service, but never used for a direct redirect).
//
// Players without the plugin (or the service) get no answer, so nothing changes for them.
// Nothing is sent to a lobby server before its service answered a probe, so on servers without
// it (or with another solution, e.g. nfsu.online's relay plugin) this plugin stays silent.
//
// Hooks: the game's own import table (IAT). The exe imports WS2_32 statically BY ORDINAL;
// the slots are identical in all v1.4 exes (NA, EU, RU). Patching them only catches the
// game's calls - not other plugins, not the OS, not this plugin's own calls.
//
// Log (off by default): [HolePunch] Log = 1 -> NFSUServerChanger_HolePunch.log next to the .asi.

#pragma once

#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <cstdio>
#include <cstdarg>
#include <map>
#include <string>
#include <vector>

#pragma comment(lib, "ws2_32.lib")

namespace HolePunch {

typedef SOCKET(WINAPI* socket_t)(int, int, int);
typedef int(WINAPI* connect_t)(SOCKET, const sockaddr*, int);
typedef int(WINAPI* bind_t)(SOCKET, const sockaddr*, int);
typedef int(WINAPI* sendto_t)(SOCKET, const char*, int, int, const sockaddr*, int);
typedef int(WINAPI* recvfrom_t)(SOCKET, char*, int, int, sockaddr*, int*);
typedef int(WINAPI* recv_t)(SOCKET, char*, int, int);
typedef int(WINAPI* closesocket_t)(SOCKET);

// WS2_32 slots in the game's IAT (all v1.4 exes).
const uintptr_t iatSocket = 0x6973C0;
const uintptr_t iatBind = 0x6973C8;
const uintptr_t iatConnect = 0x6973CC;
const uintptr_t iatClosesocket = 0x6973E0;
const uintptr_t iatSendto = 0x6973EC;
const uintptr_t iatRecvfrom = 0x6973F4;
const uintptr_t iatRecv = 0x6973F8;

const unsigned short GAME_PORT = 3658;
const unsigned short RENDEZVOUS_PORT = 10910;
const unsigned short LOBBY_PORTS[] = { 10900, 10901 };
const DWORD KEEPALIVE_MS = 15000;
const DWORD QUERY_RETRY_MS = 500;
const int QUERY_TRIES = 20; // 10 s after the race start
const DWORD RELAY_AFTER_MS = 2500; // the game gives up after about 10 s
const char RELAY_MAGIC[4] = { 'N', 'H', 'P', 'R' };
const int RELAY_HEADER = 8; // magic + pair token
const int PROBE_TRIES = 5;
const DWORD PROBE_RETRY_MS = 1000;

// What was in the slots before we patched them, called through.
static socket_t g_socket;
static connect_t g_connect;
static bind_t g_bind;
static sendto_t g_sendto;
static recvfrom_t g_recvfrom;
static recv_t g_recv;
static closesocket_t g_closesocket;

// --- Log ---

static bool g_log = false;
static CRITICAL_SECTION g_logLock;
static char g_logPath[MAX_PATH] = "NFSUServerChanger_HolePunch.log";

// Next to this .asi - the game changes its current directory after start.
static void InitLogPath()
{
    HMODULE module = NULL;
    if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        (LPCSTR)&InitLogPath, &module))
    {
        char path[MAX_PATH];
        DWORD len = GetModuleFileNameA(module, path, sizeof(path));
        char* slash = strrchr(path, '\\');
        if (len > 0 && len < sizeof(path) && slash && (size_t)(slash - path) + 40 < sizeof(path))
        {
            strcpy(slash + 1, "NFSUServerChanger_HolePunch.log");
            strcpy(g_logPath, path);
        }
    }
}

static void LogLine(const char* fmt, ...)
{
    if (!g_log) return;
    EnterCriticalSection(&g_logLock);
    FILE* f = fopen(g_logPath, "a");
    if (f)
    {
        SYSTEMTIME t;
        GetLocalTime(&t);
        fprintf(f, "[%02d:%02d:%02d.%03d] ", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds);

        va_list args;
        va_start(args, fmt);
        vfprintf(f, fmt, args);
        va_end(args);

        fprintf(f, "\n");
        fclose(f);
    }
    LeaveCriticalSection(&g_logLock);
}

static std::string AddrToString(const sockaddr* addr)
{
    if (!addr || addr->sa_family != AF_INET) return "<non-IPv4 or null>";
    const sockaddr_in* in4 = (const sockaddr_in*)addr;
    char ipStr[32];
    inet_ntop(AF_INET, &in4->sin_addr, ipStr, sizeof(ipStr));
    char out[48];
    _snprintf(out, sizeof(out), "%s:%u", ipStr, ntohs(in4->sin_port));
    out[sizeof(out) - 1] = 0;
    return out;
}

static std::string IpToString(ULONG ip)
{
    in_addr a;
    a.s_addr = ip;
    char ipStr[32];
    inet_ntop(AF_INET, &a, ipStr, sizeof(ipStr));
    return ipStr;
}

// --- Shared state (game thread + worker thread) ---

struct Peer
{
    std::string persona;
    ULONG ip = 0;              // public IP from the lobby (ADDRn), network order
    unsigned short port = 0;   // real public port from the rendezvous service, 0 = unknown
    int queriesLeft = 0;
    DWORD nextQuery = 0;
    DWORD raceStart = 0;       // when +ses arrived
    bool directSeen = false;   // a packet came straight from the peer
    bool relay = false;        // packets for this peer go through the service
    unsigned int token = 0;    // pair token for the relay, same on both sides
    bool hasPlugin = false;    // said HELLO (maybe from another IP) - the relay can be used
    ULONG localIp = 0;         // the host's MYIP (private), used by the game when both
                               // players share a public IP; network order, 0 = none
    ULONG gameDest = 0;        // where the game last sent this peer's packets (ip or localIp)
};

static CRITICAL_SECTION g_lock;
static SOCKET g_gameSocket = INVALID_SOCKET; // the game's UDP socket on GAME_PORT
static ULONG g_lobbyIp = 0;                  // where the game's lobby connection goes
static std::string g_persona;                // our own persona
static std::vector<Peer> g_peers;            // players of the current race
static std::map<SOCKET, std::string> g_lobbyStreams; // lobby TCP sockets -> unparsed bytes
static DWORD g_nextHello = 0;
static HANDLE g_wakeWorker;
static bool g_serviceOk = false; // the service on g_lobbyIp answered a probe
static int g_probesLeft = 0;
static DWORD g_nextProbe = 0;

// Same on both sides of a host<->client pair: the game and the two personas, in sorted order.
static unsigned int PairToken(const std::string& game, const std::string& a, const std::string& b)
{
    std::string key = game + "|" + (a < b ? a + "|" + b : b + "|" + a);
    unsigned int hash = 2166136261u; // FNV-1a
    for (unsigned char c : key)
    {
        hash ^= c;
        hash *= 16777619u;
    }
    return hash;
}

static bool IsLobbyPort(unsigned short port)
{
    for (unsigned short p : LOBBY_PORTS)
    {
        if (p == port) return true;
    }
    return false;
}

// --- Lobby parsing ---

// "KEY=value" fields of a lobby message body (separated by tab or newline, both are used).
static std::map<std::string, std::string> ParseFields(const char* body, size_t len)
{
    std::map<std::string, std::string> fields;
    std::string current;
    for (size_t i = 0; i <= len; i++)
    {
        char c = (i < len) ? body[i] : '\n';
        if (c == '\t' || c == '\n' || c == '\0')
        {
            size_t eq = current.find('=');
            if (eq != std::string::npos)
            {
                fields[current.substr(0, eq)] = current.substr(eq + 1);
            }
            current.clear();
        }
        else if (c != '\r')
        {
            current += c;
        }
    }
    return fields;
}

// A complete lobby message from the server. Called with g_lock held.
static void HandleLobbyMessage(const char* cmd, const char* body, size_t len)
{
    if (memcmp(cmd, "pers", 4) == 0)
    {
        std::map<std::string, std::string> fields = ParseFields(body, len);
        auto it = fields.find("PERS");
        if (it != fields.end() && it->second != g_persona)
        {
            g_persona = it->second;
            g_nextHello = 0; // HELLO right away under the new name
            LogLine("lobby: persona is %s", g_persona.c_str());
            SetEvent(g_wakeWorker);
        }
    }
    else if (memcmp(cmd, "+ses", 4) == 0)
    {
        std::map<std::string, std::string> fields = ParseFields(body, len);
        std::string self = fields.count("SELF") ? fields["SELF"] : g_persona;
        if (!self.empty() && self != g_persona)
        {
            g_persona = self;
            g_nextHello = 0;
        }
        int count = fields.count("COUNT") ? atoi(fields["COUNT"].c_str()) : 0;
        std::string game = fields.count("IDENT") ? fields["IDENT"] : fields["NAME"];
        std::string host = fields["HOST"];
        // The host's private IP, URL-encoded in PARAMS ("...MYIP%3d-1062686188"), a signed
        // 32-bit value in host byte order. Players with the same public IP use it instead of ADDRn.
        ULONG hostLocalIp = 0;
        size_t myip = fields["PARAMS"].find("MYIP%3d");
        if (myip != std::string::npos)
        {
            long value = strtol(fields["PARAMS"].c_str() + myip + 7, NULL, 10);
            hostLocalIp = htonl((ULONG)value);
        }
        DWORD now = GetTickCount();

        g_peers.clear();
        for (int n = 0; n < count; n++)
        {
            char key[16];
            _snprintf(key, sizeof(key), "OPPO%d", n);
            std::string persona = fields[key];
            _snprintf(key, sizeof(key), "ADDR%d", n);
            std::string addr = fields[key];
            if (persona.empty() || persona == self) continue;

            Peer peer;
            peer.persona = persona;
            inet_pton(AF_INET, addr.c_str(), &peer.ip);
            peer.queriesLeft = QUERY_TRIES;
            peer.nextQuery = 0;
            peer.raceStart = now;
            peer.token = PairToken(game, self, persona);
            peer.gameDest = peer.ip;
            if (persona == host && hostLocalIp != 0 && hostLocalIp != peer.ip)
            {
                peer.localIp = hostLocalIp;
                LogLine("lobby: host %s has the local address %s", persona.c_str(), IpToString(hostLocalIp).c_str());
            }
            g_peers.push_back(peer);
            LogLine("lobby: +ses peer %d: %s at %s", n, persona.c_str(), addr.c_str());
        }
        LogLine("lobby: +ses as %s, %d peer(s)", self.c_str(), (int)g_peers.size());
        if (!g_serviceOk)
        {
            g_probesLeft = PROBE_TRIES; // maybe the first probes got lost
            g_nextProbe = 0;
        }
        SetEvent(g_wakeWorker);
    }
}

// Bytes the game received on a lobby socket; messages are a 12-byte header (4-byte command,
// 4 bytes, 4-byte big-endian total length) plus body, and may arrive split or bundled.
static void FeedLobbyStream(SOCKET s, const char* data, int len)
{
    EnterCriticalSection(&g_lock);
    std::string& buffer = g_lobbyStreams[s];
    buffer.append(data, len);
    while (buffer.size() >= 12)
    {
        unsigned int total = ((unsigned char)buffer[8] << 24) | ((unsigned char)buffer[9] << 16)
            | ((unsigned char)buffer[10] << 8) | (unsigned char)buffer[11];
        if (total < 12 || total > 65536)
        {
            buffer.clear(); // out of sync - drop it, the next message starts fresh
            break;
        }
        if (buffer.size() < total) break;
        HandleLobbyMessage(buffer.data(), buffer.data() + 12, total - 12);
        buffer.erase(0, total);
    }
    LeaveCriticalSection(&g_lock);
}

// --- Hooks ---

static DWORD WINAPI Worker(LPVOID);
static volatile LONG g_workerStarted = 0;

// The worker is started with the game's first socket, not while the .asi is loaded: a game
// instance that quits right away (e.g. started twice) would otherwise unload this module
// while the new thread is still starting up in it.
static void StartWorker()
{
    if (InterlockedCompareExchange(&g_workerStarted, 1, 0) == 0)
    {
        HANDLE worker = CreateThread(NULL, 0, Worker, NULL, 0, NULL);
        if (worker) CloseHandle(worker);
    }
}

static SOCKET WINAPI Hooked_socket(int af, int type, int protocol)
{
    StartWorker();
    SOCKET s = g_socket(af, type, protocol);
    LogLine("socket() type=%s -> %zu", type == SOCK_DGRAM ? "UDP" : type == SOCK_STREAM ? "TCP" : "other", (size_t)s);
    return s;
}

static int WINAPI Hooked_connect(SOCKET s, const sockaddr* name, int namelen)
{
    int result = g_connect(s, name, namelen);
    int error = (result == SOCKET_ERROR) ? WSAGetLastError() : 0;

    LogLine("connect() socket=%zu -> %s : %d (WSAGetLastError=%d)", (size_t)s, AddrToString(name).c_str(), result, error);
    if (name && name->sa_family == AF_INET && IsLobbyPort(ntohs(((const sockaddr_in*)name)->sin_port)))
    {
        EnterCriticalSection(&g_lock);
        ULONG ip = ((const sockaddr_in*)name)->sin_addr.s_addr;
        if (ip != g_lobbyIp)
        {
            g_lobbyIp = ip;
            g_serviceOk = false;
        }
        if (!g_serviceOk)
        {
            g_probesLeft = PROBE_TRIES;
            g_nextProbe = 0;
        }
        g_lobbyStreams[s].clear();
        LeaveCriticalSection(&g_lock);
        SetEvent(g_wakeWorker);
    }

    // Our bookkeeping must not change what the game sees afterwards.
    if (error) WSASetLastError(error);
    return result;
}

static int WINAPI Hooked_bind(SOCKET s, const sockaddr* name, int namelen)
{
    int result = g_bind(s, name, namelen);
    int error = (result == SOCKET_ERROR) ? WSAGetLastError() : 0;

    LogLine("bind() socket=%zu %s : %d (WSAGetLastError=%d)", (size_t)s, AddrToString(name).c_str(), result, error);
    if (result == 0 && name && name->sa_family == AF_INET && ntohs(((const sockaddr_in*)name)->sin_port) == GAME_PORT)
    {
        EnterCriticalSection(&g_lock);
        g_gameSocket = s;
        g_nextHello = 0;
        LeaveCriticalSection(&g_lock);
        LogLine("game UDP socket is %zu", (size_t)s);
        SetEvent(g_wakeWorker);
    }

    if (error) WSASetLastError(error);
    return result;
}

static int WINAPI Hooked_sendto(SOCKET s, const char* buf, int len, int flags, const sockaddr* to, int tolen)
{
    if (to && to->sa_family == AF_INET && tolen >= (int)sizeof(sockaddr_in)
        && ntohs(((const sockaddr_in*)to)->sin_port) == GAME_PORT)
    {
        const sockaddr_in* dest = (const sockaddr_in*)to;
        unsigned short realPort = 0;
        bool relay = false;
        unsigned int token = 0;
        ULONG relayIp = 0;
        EnterCriticalSection(&g_lock);
        for (Peer& peer : g_peers)
        {
            bool publicIp = peer.ip == dest->sin_addr.s_addr;
            if (publicIp || (peer.localIp != 0 && peer.localIp == dest->sin_addr.s_addr))
            {
                realPort = publicIp ? peer.port : 0; // a redirect only applies to the public address
                relay = peer.relay;
                token = peer.token;
                peer.gameDest = dest->sin_addr.s_addr;
            }
        }
        relayIp = g_lobbyIp;
        LeaveCriticalSection(&g_lock);

        char wrapped[2048];
        if (relay && relayIp != 0 && len >= 0 && len + RELAY_HEADER <= (int)sizeof(wrapped))
        {
            memcpy(wrapped, RELAY_MAGIC, 4);
            memcpy(wrapped + 4, &token, 4);
            memcpy(wrapped + RELAY_HEADER, buf, len);
            sockaddr_in service = {};
            service.sin_family = AF_INET;
            service.sin_addr.s_addr = relayIp;
            service.sin_port = htons(RENDEZVOUS_PORT);
            static int loggedRelay = 0;
            if (loggedRelay < 20)
            {
                loggedRelay++;
                LogLine("sendto() %s -> via relay (type %02X, len %d)",
                    AddrToString(to).c_str(), len > 0 ? (unsigned char)buf[0] : 0, len);
            }
            int sent = g_sendto(s, wrapped, len + RELAY_HEADER, flags, (const sockaddr*)&service, sizeof(service));
            // The game sent `len` bytes as far as it's concerned.
            return (sent == SOCKET_ERROR) ? sent : len;
        }

        if (realPort != 0 && realPort != GAME_PORT)
        {
            sockaddr_in redirected = *dest;
            redirected.sin_port = htons(realPort);
            static int logged = 0;
            if (logged < 20)
            {
                logged++;
                LogLine("sendto() %s -> redirected to port %u (type %02X, len %d)",
                    AddrToString(to).c_str(), realPort, len > 0 ? (unsigned char)buf[0] : 0, len);
            }
            return g_sendto(s, buf, len, flags, (const sockaddr*)&redirected, sizeof(redirected));
        }

        static int loggedDirect = 0;
        if (loggedDirect < 20)
        {
            loggedDirect++;
            LogLine("sendto() socket=%zu -> %s unchanged (type %02X, len %d)",
                (size_t)s, AddrToString(to).c_str(), len > 0 ? (unsigned char)buf[0] : 0, len);
        }
    }
    return g_sendto(s, buf, len, flags, to, tolen);
}

static int WINAPI Hooked_recvfrom(SOCKET s, char* buf, int len, int flags, sockaddr* from, int* fromlen)
{
    int result = g_recvfrom(s, buf, len, flags, from, fromlen);
    if (result == SOCKET_ERROR || !from || !fromlen || *fromlen < (int)sizeof(sockaddr_in) || from->sa_family != AF_INET)
    {
        return result;
    }

    sockaddr_in* src = (sockaddr_in*)from;
    unsigned short srcPort = ntohs(src->sin_port);

    // Relayed: unwrap and report it as coming from the peer (ADDRn:3658).
    if (result >= RELAY_HEADER && srcPort == RENDEZVOUS_PORT && memcmp(buf, RELAY_MAGIC, 4) == 0)
    {
        unsigned int token;
        memcpy(&token, buf + 4, 4);
        ULONG peerIp = 0;
        bool switched = false;
        EnterCriticalSection(&g_lock);
        if (src->sin_addr.s_addr == g_lobbyIp)
        {
            for (Peer& peer : g_peers)
            {
                if (peer.token == token)
                {
                    peerIp = peer.gameDest ? peer.gameDest : peer.ip;
                    if (!peer.relay)
                    {
                        peer.relay = true; // the peer uses the relay - so do we
                        switched = true;
                    }
                }
            }
        }
        LeaveCriticalSection(&g_lock);

        if (peerIp != 0)
        {
            if (switched) LogLine("relay: packets from %s arrive via relay - using it too", IpToString(peerIp).c_str());
            memmove(buf, buf + RELAY_HEADER, result - RELAY_HEADER);
            src->sin_addr.s_addr = peerIp;
            src->sin_port = htons(GAME_PORT);
            return result - RELAY_HEADER;
        }
        // Not for a current peer: nothing the game should see.
        WSASetLastError(WSAEWOULDBLOCK);
        return SOCKET_ERROR;
    }

    // Anything straight from a peer on the game's socket means the direct way works (at least
    // this direction). Other sockets don't count - e.g. the game's pings to the same IP.
    EnterCriticalSection(&g_lock);
    for (Peer& peer : g_peers)
    {
        if (s == g_gameSocket && !peer.directSeen
            && (peer.ip == src->sin_addr.s_addr || (peer.localIp != 0 && peer.localIp == src->sin_addr.s_addr)))
        {
            peer.directSeen = true;
            LogLine("direct: first packet from %s", AddrToString(from).c_str());
        }
    }
    LeaveCriticalSection(&g_lock);

    // A peer answering from its real port: report it to the game as ADDRn:3658, the address
    // it expects (and sends its replies to, which sendto() redirects again).
    if (srcPort != GAME_PORT)
    {
        bool known = false;
        EnterCriticalSection(&g_lock);
        for (const Peer& peer : g_peers)
        {
            if (peer.ip == src->sin_addr.s_addr && peer.port == srcPort) known = true;
        }
        LeaveCriticalSection(&g_lock);

        if (known)
        {
            static int logged = 0;
            if (logged < 20)
            {
                logged++;
                LogLine("recvfrom() %s -> reported as port %u (type %02X, len %d)",
                    AddrToString(from).c_str(), GAME_PORT, result > 0 ? (unsigned char)buf[0] : 0, result);
            }
            src->sin_port = htons(GAME_PORT);
            return result;
        }
    }

    static int loggedOther = 0;
    if (loggedOther < 20)
    {
        loggedOther++;
        LogLine("recvfrom() socket=%zu <- %s unchanged (type %02X, len %d)",
            (size_t)s, AddrToString(from).c_str(), result > 0 ? (unsigned char)buf[0] : 0, result);
    }
    return result;
}

static int WINAPI Hooked_recv(SOCKET s, char* buf, int len, int flags)
{
    int result = g_recv(s, buf, len, flags);
    int error = (result == SOCKET_ERROR) ? WSAGetLastError() : 0;

    if (result > 0)
    {
        bool lobby;
        EnterCriticalSection(&g_lock);
        lobby = g_lobbyStreams.count(s) > 0;
        LeaveCriticalSection(&g_lock);
        if (lobby) FeedLobbyStream(s, buf, result);
    }

    if (error) WSASetLastError(error);
    return result;
}

static int WINAPI Hooked_closesocket(SOCKET s)
{
    EnterCriticalSection(&g_lock);
    if (s == g_gameSocket)
    {
        g_gameSocket = INVALID_SOCKET;
        LogLine("game UDP socket %zu closed", (size_t)s);
    }
    g_lobbyStreams.erase(s);
    LeaveCriticalSection(&g_lock);
    return g_closesocket(s);
}

// --- Worker: HELLO keepalive and QUERY ---

static DWORD WINAPI Worker(LPVOID)
{
    WSADATA wsa;
    WSAStartup(MAKEWORD(2, 2), &wsa);

    // Our own socket for queries, so the answers never reach the game.
    SOCKET querySocket = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    u_long nonBlocking = 1;
    ioctlsocket(querySocket, FIONBIO, &nonBlocking);

    for (;;)
    {
        WaitForSingleObject(g_wakeWorker, 200);
        DWORD now = GetTickCount();

        EnterCriticalSection(&g_lock);
        SOCKET gameUdpSocket = g_gameSocket;
        ULONG lobbyIp = g_lobbyIp;
        std::string persona = g_persona;
        bool serviceOk = g_serviceOk;
        bool probeDue = !serviceOk && lobbyIp != 0 && g_probesLeft > 0
            && (g_nextProbe == 0 || (int)(now - g_nextProbe) >= 0);
        if (probeDue)
        {
            g_probesLeft--;
            g_nextProbe = now + PROBE_RETRY_MS;
        }
        bool helloDue = serviceOk && gameUdpSocket != INVALID_SOCKET && lobbyIp != 0 && !persona.empty()
            && (g_nextHello == 0 || (int)(now - g_nextHello) >= 0);
        if (helloDue) g_nextHello = now + KEEPALIVE_MS;

        std::vector<std::pair<ULONG, std::string>> queries;
        for (Peer& peer : g_peers)
        {
            // Hole punching didn't get through: relay, but only to a peer with the plugin.
            if (!peer.relay && !peer.directSeen && (peer.port != 0 || peer.hasPlugin)
                && (int)(now - peer.raceStart) >= (int)RELAY_AFTER_MS)
            {
                peer.relay = true;
                LogLine("relay: nothing from %s after %lu ms - switching to the relay",
                    peer.persona.c_str(), (unsigned long)RELAY_AFTER_MS);
            }

            if (serviceOk && peer.port == 0 && !peer.hasPlugin && peer.queriesLeft > 0
                && (peer.nextQuery == 0 || (int)(now - peer.nextQuery) >= 0))
            {
                peer.queriesLeft--;
                peer.nextQuery = now + QUERY_RETRY_MS;
                queries.push_back({ peer.ip, peer.persona });
                if (peer.queriesLeft == 0)
                {
                    LogLine("query: no answer for %s - stays on port %u", peer.persona.c_str(), GAME_PORT);
                }
            }
        }
        LeaveCriticalSection(&g_lock);

        if (lobbyIp == 0) continue;
        sockaddr_in service = {};
        service.sin_family = AF_INET;
        service.sin_addr.s_addr = lobbyIp;
        service.sin_port = htons(RENDEZVOUS_PORT);

        // Is there a service at all? Asked from our own socket; any answer will do.
        if (probeDue)
        {
            const char probe[] = "NHP1 QUERY 0.0.0.0 ?";
            sendto(querySocket, probe, (int)sizeof(probe) - 1, 0, (const sockaddr*)&service, sizeof(service));
        }

        // From the GAME's socket, so the service sees the race's NAT mapping. This plugin's own
        // sendto isn't hooked, so it goes straight to ws2_32.
        if (helloDue)
        {
            std::string hello = "NHP1 HELLO " + persona;
            int sent = sendto(gameUdpSocket, hello.c_str(), (int)hello.size(), 0, (const sockaddr*)&service, sizeof(service));
            LogLine("HELLO as %s to %s : %d", persona.c_str(), AddrToString((const sockaddr*)&service).c_str(), sent);
        }

        for (const auto& query : queries)
        {
            std::string msg = "NHP1 QUERY " + IpToString(query.first) + " " + query.second;
            sendto(querySocket, msg.c_str(), (int)msg.size(), 0, (const sockaddr*)&service, sizeof(service));
        }

        // Answers: "NHP1 FOUND <ip> <port> <persona>" / "NHP1 OTHER <ip> <persona>" / "NHP1 NONE <ip> <persona>"
        char answer[512];
        sockaddr_in from;
        int fromLen = sizeof(from);
        int got;
        while ((got = recvfrom(querySocket, answer, sizeof(answer) - 1, 0, (sockaddr*)&from, &fromLen)) > 0)
        {
            answer[got] = 0;
            fromLen = sizeof(from);
            char ipStr[32];
            unsigned int port = 0;
            int nameOffset = 0;
            if (from.sin_addr.s_addr != lobbyIp || strncmp(answer, "NHP1 ", 5) != 0) continue;

            EnterCriticalSection(&g_lock);
            if (!g_serviceOk && g_lobbyIp == lobbyIp)
            {
                g_serviceOk = true;
                g_nextHello = 0;
                LogLine("service: found on %s:%u", IpToString(lobbyIp).c_str(), RENDEZVOUS_PORT);
                SetEvent(g_wakeWorker);
            }
            LeaveCriticalSection(&g_lock);

            // The persona has the plugin, but its UDP comes from another IP than the lobby's.
            if (sscanf(answer, "NHP1 OTHER %31s %n", ipStr, &nameOffset) == 1 && nameOffset > 0)
            {
                ULONG ip = 0;
                inet_pton(AF_INET, ipStr, &ip);
                std::string name = answer + nameOffset;
                EnterCriticalSection(&g_lock);
                for (Peer& peer : g_peers)
                {
                    if (peer.persona == name && peer.ip == ip && !peer.hasPlugin)
                    {
                        peer.hasPlugin = true;
                        LogLine("query: %s has the plugin, but its UDP comes from another IP - relay only", name.c_str());
                    }
                }
                LeaveCriticalSection(&g_lock);
                continue;
            }

            nameOffset = 0;
            if (sscanf(answer, "NHP1 FOUND %31s %u %n", ipStr, &port, &nameOffset) < 2 || nameOffset == 0)
            {
                continue;
            }
            ULONG ip = 0;
            inet_pton(AF_INET, ipStr, &ip);
            std::string name = answer + nameOffset;

            EnterCriticalSection(&g_lock);
            for (Peer& peer : g_peers)
            {
                // Only the port, and only for the IP the lobby reported.
                if (peer.persona == name && peer.ip == ip && peer.port == 0 && port > 0 && port < 65536)
                {
                    peer.port = (unsigned short)port;
                    peer.hasPlugin = true;
                    LogLine("query: %s is at %s:%u%s", name.c_str(), ipStr, port,
                        port == GAME_PORT ? " (same as the default, nothing to redirect)" : "");
                }
            }
            LeaveCriticalSection(&g_lock);
        }
    }
}

// --- Setup ---

// Name of the module the address belongs to, for the log.
static std::string ModuleOf(void* addr)
{
    HMODULE module = NULL;
    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        (LPCSTR)addr, &module))
    {
        return "<no module>";
    }
    char path[MAX_PATH];
    GetModuleFileNameA(module, path, sizeof(path));
    const char* name = strrchr(path, '\\');
    return name ? name + 1 : path;
}

// Replaces the function pointer in an IAT slot; returns what was there (called through, so
// another plugin that patched the slot first keeps working).
static void* PatchSlot(const char* name, uintptr_t slot, void* hookFn)
{
    void** entry = reinterpret_cast<void**>(slot);
    void* previous = *entry;

    DWORD oldProtect;
    if (!previous || !VirtualProtect(entry, sizeof(void*), PAGE_READWRITE, &oldProtect))
    {
        LogLine("  %s slot 0x%06X (%p in %s) - NOT patched", name, (unsigned)slot, previous, ModuleOf(previous).c_str());
        return nullptr;
    }
    *entry = hookFn;
    VirtualProtect(entry, sizeof(void*), oldProtect, &oldProtect);

    LogLine("  %s slot 0x%06X: was %p in %s - patched", name, (unsigned)slot, previous, ModuleOf(previous).c_str());
    return previous;
}

// log: write NFSUServerChanger_HolePunch.log. Called from Init() (supported exes only).
inline void Install(bool log)
{
    g_log = log;
    InitializeCriticalSection(&g_logLock);
    InitializeCriticalSection(&g_lock);
    InitLogPath();
    LogLine("=== hole punching attached (pid=%lu) ===", GetCurrentProcessId());

    LogLine("Patching the game's WS2_32 IAT slots:");
    socket_t socketFn = (socket_t)PatchSlot("socket", iatSocket, (void*)Hooked_socket);
    connect_t connectFn = (connect_t)PatchSlot("connect", iatConnect, (void*)Hooked_connect);
    bind_t bindFn = (bind_t)PatchSlot("bind", iatBind, (void*)Hooked_bind);
    sendto_t sendtoFn = (sendto_t)PatchSlot("sendto", iatSendto, (void*)Hooked_sendto);
    recvfrom_t recvfromFn = (recvfrom_t)PatchSlot("recvfrom", iatRecvfrom, (void*)Hooked_recvfrom);
    recv_t recvFn = (recv_t)PatchSlot("recv", iatRecv, (void*)Hooked_recv);
    closesocket_t closesocketFn = (closesocket_t)PatchSlot("closesocket", iatClosesocket, (void*)Hooked_closesocket);

    // The hooks call through these; set before the game can call a hook (it can't yet:
    // we're still inside DllMain, before the game's main code runs).
    g_socket = socketFn;
    g_connect = connectFn;
    g_bind = bindFn;
    g_sendto = sendtoFn;
    g_recvfrom = recvfromFn;
    g_recv = recvFn;
    g_closesocket = closesocketFn;

    // The worker itself starts with the game's first socket (StartWorker): it only acts once
    // the game has a socket and a lobby connection anyway.
    g_wakeWorker = CreateEventA(NULL, FALSE, FALSE, NULL);
}

} // namespace HolePunch
