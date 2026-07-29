//! Socket discovery: which process is sitting on which port.

use std::collections::{HashMap, HashSet};
use std::fmt;

use anyhow::{Context, Result};
use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};

use crate::process;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

impl fmt::Display for Proto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        })
    }
}

/// One process holding one port.
#[derive(Clone)]
pub struct PortEntry {
    pub port: u16,
    pub proto: Proto,
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub memory: u64,
}

impl PortEntry {
    /// The text the fuzzy filter matches against.
    pub fn haystack(&self) -> String {
        format!("{} {} {} {}", self.port, self.name, self.user, self.proto)
    }
}

/// Every listening TCP socket and every bound UDP socket, sorted by port.
pub fn scan() -> Result<Vec<PortEntry>> {
    collect(None)
}

/// Just the sockets bound to `port`.
pub fn scan_port(port: u16) -> Result<Vec<PortEntry>> {
    collect(Some(port))
}

fn collect(only: Option<u16>) -> Result<Vec<PortEntry>> {
    let sockets = netstat2::get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .context("failed to enumerate network sockets")?;

    let mut entries = Vec::new();
    // A process that binds both 0.0.0.0 and :: shows up once per address
    // family; the user only cares that it holds the port.
    let mut seen = HashSet::new();
    // Several sockets usually belong to the same process — resolve each PID once.
    let mut cache: HashMap<u32, Option<(String, String, u64)>> = HashMap::new();

    for socket in sockets {
        let proto = match &socket.protocol_socket_info {
            // A TCP socket in any other state is a connection, not an owner
            // of the port in the sense that matters here.
            ProtocolSocketInfo::Tcp(tcp) if tcp.state == TcpState::Listen => Proto::Tcp,
            ProtocolSocketInfo::Tcp(_) => continue,
            ProtocolSocketInfo::Udp(_) => Proto::Udp,
        };
        let port = socket.local_port();
        // Port 0 means "the kernel didn't assign one" — an unbound UDP socket.
        // Nobody ever needs to free port 0.
        if port == 0 {
            continue;
        }
        if only.is_some_and(|wanted| wanted != port) {
            continue;
        }

        for pid in socket.associated_pids {
            if !seen.insert((port, proto, pid)) {
                continue;
            }
            let details = cache.entry(pid).or_insert_with(|| {
                process::info(pid).map(|info| (info.name, process::username(info.uid), info.rss))
            });
            // PIDs we can't inspect (usually root-owned, and we aren't) are
            // still worth listing — the port is occupied either way.
            let (name, user, memory) = details
                .clone()
                .unwrap_or_else(|| ("?".to_owned(), "?".to_owned(), 0));

            entries.push(PortEntry {
                port,
                proto,
                pid,
                name,
                user,
                memory,
            });
        }
    }

    entries.sort_unstable_by_key(|e| (e.port, e.pid));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn finds_a_socket_we_just_opened() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let found = scan_port(port).unwrap();
        let me = std::process::id();
        let ours = found
            .iter()
            .find(|e| e.pid == me)
            .expect("our own listener should be discoverable");

        assert_eq!(ours.port, port);
        assert!(matches!(ours.proto, Proto::Tcp));
        assert_ne!(ours.name, "?", "we can always inspect our own process");
    }

    #[test]
    fn closed_port_yields_nothing() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
            // listener dropped here, freeing the port
        };
        assert!(scan_port(port).unwrap().is_empty());
    }

    #[test]
    fn unbound_sockets_are_not_listed() {
        assert!(
            scan().unwrap().iter().all(|e| e.port != 0),
            "port 0 is not a port anyone can free"
        );
        assert!(scan_port(0).unwrap().is_empty());
    }

    #[test]
    fn entries_come_back_sorted_and_deduped() {
        let entries = scan().unwrap();
        assert!(
            entries
                .windows(2)
                .all(|w| (w[0].port, w[0].pid) <= (w[1].port, w[1].pid))
        );

        let mut seen = HashSet::new();
        assert!(
            entries
                .iter()
                .all(|e| seen.insert((e.port, e.proto, e.pid))),
            "the same process/port/proto must not be listed twice"
        );
    }
}
