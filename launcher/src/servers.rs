//! Server list, built locally: [FixedServers] from NFSUServerChanger.ini + public lists
//! (trackers) + servers.cache + servers.dat, each server queried directly.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Server;

pub const CACHE_NAME: &str = "servers.cache";
// Lobby servers answer on this port with their status (same as nfs.onl's servers.php/kickStart use).
const STATUS_PORT: u16 = 10980;
// EA Nation has no status port; it echoes an "@tic" packet on this one.
const EA_NATION_PORT: u16 = 5000;

/// One server as known from the cache or a direct query.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub host: String,
    pub ip: String,
    pub title: String,
    pub online: i32,
    pub has_stats: bool,
    pub reachable: bool,
    pub ea_nation: bool,
    /// Only in the list because of servers.dat - not written to the cache.
    #[serde(skip)]
    pub custom_only: bool,
    /// Not queried yet in this run and nothing cached about it.
    #[serde(skip)]
    pub pending: bool,
    /// A default server or listed by a public list in this run.
    #[serde(skip)]
    pub listed: bool,
}

pub fn load_cache(path: &Path) -> Vec<Entry> {
    fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Servers no public list mentions anymore and that are offline are dropped -
/// unless the public lists couldn't be fetched (`prune` false).
pub fn save_cache(path: &Path, servers: &[Entry], prune: bool) {
    let servers: Vec<&Entry> = servers
        .iter()
        .filter(|e| !e.custom_only && (e.listed || e.reachable || !prune))
        .collect();
    if let Ok(json) = serde_json::to_vec_pretty(&servers) {
        let _ = fs::write(path, json);
    }
}

/// "name:port" -> ("name", Some(port)) if it ends in ':' + a number 1-65535 (a lobby
/// port for the plugin); anything else is just a name. Same rule as the plugin's.
pub fn split_host_port(host: &str) -> (&str, Option<u16>) {
    if let Some((name, port)) = host.rsplit_once(':') {
        if !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(port @ 1..) = port.parse::<u16>() {
                return (name, Some(port));
            }
        }
    }
    (host, None)
}

/// Same server, compared without a lobby port (the status query doesn't use it).
fn matches(entry: &Entry, host: &str) -> bool {
    let (name, _) = split_host_port(host);
    // A bare ":port" names no server (and unresolved entries have an empty IP).
    !name.is_empty()
        && (split_host_port(&entry.host).0.eq_ignore_ascii_case(name) || entry.ip == name)
}

/// Everything to query: cached servers, the fixed public servers, and custom
/// servers that aren't one of those already.
pub fn known_servers(
    cached: &[Entry],
    fixed: &[(String, String)],
    custom: &[String],
) -> Vec<Entry> {
    let mut list = cached.to_vec();
    for (name, host) in fixed {
        match list.iter_mut().find(|e| matches(e, host)) {
            Some(e) => e.listed = true,
            // The name shows until the server reports its own title (EA Nation never does).
            None => list.push(Entry {
                host: host.clone(),
                title: name.clone(),
                pending: true,
                listed: true,
                ..Default::default()
            }),
        }
    }
    for host in custom {
        if !list.iter().any(|e| matches(e, host)) {
            list.push(Entry {
                host: host.clone(),
                custom_only: true,
                pending: true,
                ..Default::default()
            });
        }
    }
    list
}

/// Fills in every entry's IP (in parallel), see `probe`.
pub fn resolve_ips(list: &mut [Entry]) {
    std::thread::scope(|s| {
        for entry in list.iter_mut() {
            s.spawn(move || {
                if let Some(addr) = resolve(&entry.host, STATUS_PORT) {
                    entry.ip = addr.ip().to_string();
                }
            });
        }
    });
}

/// IPs from all public lists (http://<provider>/tracker/get_list.php), merged
/// and deduplicated - like getServerlists() in nfs.onl's servers.php.
/// None if no provider answered.
pub fn fetch_public_lists(providers: &[String]) -> Option<Vec<String>> {
    let lists: Vec<Option<Vec<String>>> = std::thread::scope(|s| {
        let handles: Vec<_> = providers
            .iter()
            .map(|p| s.spawn(move || fetch_public_list(p)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    if lists.iter().all(Option::is_none) {
        return None;
    }
    let mut ips: Vec<String> = Vec::new();
    for ip in lists.into_iter().flatten().flatten() {
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    Some(ips)
}

/// `provider` is a hostname or a full URL. Redirects (e.g. to https) are followed.
fn fetch_public_list(provider: &str) -> Option<Vec<String>> {
    let url = if provider.contains("://") {
        provider.to_string()
    } else {
        format!("http://{provider}/tracker/get_list.php")
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let body = agent
        .get(&url)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    Some(body.lines().filter_map(decode_ip).collect())
}

/// The trackers store each IPv4 as a little-endian u32 (see decodeIP() in servers.php).
fn decode_ip(line: &str) -> Option<String> {
    let n: u32 = line.trim().parse().ok().filter(|&n| n > 0)?;
    Some(Ipv4Addr::from(n.to_le_bytes()).to_string())
}

/// Marks known servers the public lists mention and adds the ones not known yet.
pub fn add_public(list: &mut Vec<Entry>, public: Option<&[String]>) {
    for ip in public.unwrap_or_default() {
        match list.iter_mut().find(|e| matches(e, ip)) {
            Some(e) => {
                e.listed = true;
                // Public now, so it belongs in the cache even if it came from servers.dat.
                e.custom_only = false;
            }
            None => list.push(Entry {
                host: ip.clone(),
                ip: ip.clone(),
                pending: true,
                listed: true,
                ..Default::default()
            }),
        }
    }
}

/// Queries one server; keeps what was known about it if the query fails.
pub fn probe(mut entry: Entry) -> Entry {
    entry.pending = false;
    // Needed to match custom servers listed by IP; re-resolved every time so a
    // stale cached IP gets replaced.
    if let Some(addr) = resolve(&entry.host, STATUS_PORT) {
        entry.ip = addr.ip().to_string();
    }
    // Known EA Nation servers only get their own probe: their status port doesn't answer,
    // so trying it would only cost another timeout. Only if port 5000 is gone entirely,
    // it may have become a normal server, so the status port is tried below.
    let known_ea = if entry.ea_nation {
        Some(probe_ea_nation(&entry.host))
    } else {
        None
    };
    if let Some(result @ (EaProbe::Echo | EaProbe::NoReply)) = known_ea {
        entry.reachable = result == EaProbe::Echo;
        entry.has_stats = false;
    } else if let Some((title, online)) = probe_status(&entry.host) {
        entry.title = title;
        entry.online = online;
        entry.has_stats = true;
        entry.reachable = true;
        entry.ea_nation = false;
    } else {
        // No status port: maybe an EA Nation server (same fallback as kickStart).
        let result = if known_ea.is_some() {
            EaProbe::NoConnection
        } else {
            probe_ea_nation(&entry.host)
        };
        // A hanging one stays known as EA Nation, so next time it's checked quickly.
        entry.ea_nation = result != EaProbe::NoConnection;
        entry.reachable = result == EaProbe::Echo;
        entry.has_stats = false;
    }
    entry
}

/// Online servers, plus every custom server (offline ones included).
pub fn merge_custom(known: &[Entry], custom: &[String]) -> Vec<Server> {
    let mut servers: Vec<Server> = Vec::new();
    for entry in known {
        let custom_host = custom.iter().find(|h| matches(entry, h));
        if !entry.reachable && custom_host.is_none() {
            continue;
        }
        servers.push(Server {
            // Keep the list's spelling (e.g. an IP) so it can be removed again.
            host: custom_host.unwrap_or(&entry.host).as_str().into(),
            title: entry.title.as_str().into(),
            online: entry.online,
            has_stats: entry.has_stats,
            reachable: entry.reachable,
            custom: custom_host.is_some(),
            checking: entry.pending,
        });
    }
    // Online first, busiest on top; stable sort keeps the list order otherwise.
    servers.sort_by_key(|s| (!s.reachable, -s.online));
    servers
}

/// First IPv4 address: NFSU itself only speaks IPv4, and lobby servers usually
/// don't listen on IPv6 even if their hostname has an AAAA record.
fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (split_host_port(host).0, port)
        .to_socket_addrs()
        .ok()?
        .find(SocketAddr::is_ipv4)
}

fn connect(host: &str, port: u16) -> Option<TcpStream> {
    let addr = resolve(host, port)?;
    // Generous timeouts: far-away servers can take >1s to connect, and all queries run in parallel.
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    Some(stream)
}

/// Reads the lobby server's status line ("online|rooms|uptime|system|version|title|...~~~rooms").
fn probe_status(host: &str) -> Option<(String, i32)> {
    let mut stream = connect(host, STATUS_PORT)?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(3).any(|w| w == b"~~~") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let (status, _) = text.split_once("~~~")?;
    let fields: Vec<&str> = status.split('|').collect();
    (fields.len() >= 6).then(|| {
        (
            fields[5].trim().to_string(),
            fields[0].trim().parse().unwrap_or(0),
        )
    })
}

#[derive(PartialEq)]
enum EaProbe {
    /// The "@tic" echo came back: an EA Nation server that works.
    Echo,
    /// Port 5000 took the connection but nothing (valid) came back: a hanging server.
    NoReply,
    /// Port 5000 not reachable at all.
    NoConnection,
}

/// EA Nation echoes "@tic" + 4 reserved bytes + length 12 (big-endian).
fn probe_ea_nation(host: &str) -> EaProbe {
    const PROBE: [u8; 12] = [b'@', b't', b'i', b'c', 0, 0, 0, 0, 0, 0, 0, 12];
    let Some(mut stream) = connect(host, EA_NATION_PORT) else {
        return EaProbe::NoConnection;
    };
    let mut reply = [0u8; 12];
    if stream.write_all(&PROBE).is_ok() && stream.read_exact(&mut reply).is_ok() && reply == PROBE {
        EaProbe::Echo
    } else {
        EaProbe::NoReply
    }
}

#[cfg(test)]
#[path = "tests/servers.rs"]
mod tests;
