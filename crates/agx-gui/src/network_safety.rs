//! Read local TCP listeners and UDP bindings without sending network traffic.

use std::{
    collections::BTreeSet,
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Transport {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Binding {
    socket: SocketAddr,
    transport: Transport,
}

#[derive(Clone, Copy)]
enum Scope {
    Loopback,
    AllIpv4,
    AllIpv6,
    Interface,
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub score: i32,
    pub reason: &'static str,
}

const LOCAL: Profile = Profile {
    score: 0,
    reason: "No non-loopback ports were found.",
};

const EXPOSED: Profile = Profile {
    score: 40,
    reason: "A port is bound on a network address.",
};

const WILDCARD: Profile = Profile {
    score: 55,
    reason: "A port is bound to all addresses in its IP family.",
};

const SSH: Profile = Profile {
    score: 15,
    reason: "The SSH port is bound beyond loopback.",
};

const SSH_WILDCARD: Profile = Profile {
    score: 30,
    reason: "The SSH port has a wildcard bind address.",
};

const LEGACY: Profile = Profile {
    score: 80,
    reason: "A port commonly used by plaintext remote access is bound beyond loopback.",
};

const VNC: Profile = Profile {
    score: 75,
    reason: "A VNC-range port is bound beyond loopback.",
};

const INCOMPLETE: Profile = Profile {
    score: -1,
    reason: "Some local socket information is unavailable.",
};

impl Binding {
    fn scope(self) -> Scope {
        match self.socket.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => Scope::AllIpv4,
            IpAddr::V6(ip) if ip.is_unspecified() => Scope::AllIpv6,
            IpAddr::V4(ip) if ip.is_loopback() => Scope::Loopback,
            IpAddr::V6(ip)
                if ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback()) =>
            {
                Scope::Loopback
            }
            _ => Scope::Interface,
        }
    }

    fn finding(self) -> Profile {
        let scope = self.scope();
        if matches!(scope, Scope::Loopback) {
            return LOCAL;
        }
        if self.transport == Transport::Tcp {
            match self.socket.port() {
                21 | 23 => return LEGACY,
                22 if matches!(scope, Scope::AllIpv4 | Scope::AllIpv6) => return SSH_WILDCARD,
                22 => return SSH,
                5900..=5999 => return VNC,
                _ => {}
            }
        }
        match scope {
            Scope::AllIpv4 | Scope::AllIpv6 => WILDCARD,
            _ => EXPOSED,
        }
    }
}

pub(super) fn collect() -> Profile {
    let mut complete = true;
    let mut bindings = BTreeSet::new();
    for (file, transport, ipv6) in [
        ("/proc/net/tcp", Transport::Tcp, false),
        ("/proc/net/tcp6", Transport::Tcp, true),
        ("/proc/net/udp", Transport::Udp, false),
        ("/proc/net/udp6", Transport::Udp, true),
    ] {
        match fs::read_to_string(file) {
            Ok(text) => complete &= parse_table(&text, transport, ipv6, &mut bindings),
            Err(error)
                if ipv6
                    && error.kind() == std::io::ErrorKind::NotFound
                    && !std::path::Path::new("/proc/net/if_inet6").exists() => {}
            Err(_) => complete = false,
        }
    }
    if complete {
        bindings
            .into_iter()
            .map(Binding::finding)
            .max_by_key(|profile| profile.score)
            .unwrap_or(LOCAL)
    } else {
        INCOMPLETE
    }
}

fn parse_table(
    text: &str,
    transport: Transport,
    ipv6: bool,
    bindings: &mut BTreeSet<Binding>,
) -> bool {
    let mut lines = text.lines();
    if !lines
        .next()
        .is_some_and(|line| line.contains("local_address"))
    {
        return false;
    }
    let mut complete = true;
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace().skip(1);
        let (Some(local), Some(remote), Some(state)) =
            (fields.next(), fields.next(), fields.next())
        else {
            complete = false;
            continue;
        };
        let inbound = match transport {
            Transport::Tcp => state == "0A",
            Transport::Udp => state == "07" && remote.ends_with(":0000"),
        };
        if inbound {
            match parse_socket(local, ipv6) {
                Some(socket) if socket.port() != 0 => {
                    bindings.insert(Binding { socket, transport });
                }
                _ => complete = false,
            }
        }
    }
    complete
}

fn parse_socket(value: &str, ipv6: bool) -> Option<SocketAddr> {
    let (address, port) = value.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let ip = if ipv6 && address.len() == 32 && address.is_ascii() {
        let mut octets = [0; 16];
        for (index, word) in octets.chunks_exact_mut(4).enumerate() {
            let offset = index * 8;
            let value = u32::from_str_radix(&address[offset..offset + 8], 16).ok()?;
            word.copy_from_slice(&value.to_ne_bytes());
        }
        IpAddr::V6(Ipv6Addr::from(octets))
    } else if !ipv6 && address.len() == 8 {
        IpAddr::V4(Ipv4Addr::from(
            u32::from_str_radix(address, 16).ok()?.to_ne_bytes(),
        ))
    } else {
        return None;
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_inventory_ignores_established_connections_and_marks_loopback() {
        let mut bindings = BTreeSet::new();
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        let table = format!(
            "sl local_address rem_address st\n0: {loopback:08X}:170C 00000000:0000 0A\n1: 00000000:0016 00000000:0000 0A\n2: 00000000:0100 00000000:AAAA 01\n"
        );
        assert!(parse_table(&table, Transport::Tcp, false, &mut bindings));
        assert_eq!(bindings.len(), 2);
        let mut scores = bindings.into_iter().map(|binding| binding.finding().score);
        assert_eq!(scores.next(), Some(30));
        assert_eq!(scores.next(), Some(0));
        let restricted = Binding {
            socket: "10.78.0.2:22".parse().unwrap(),
            transport: Transport::Tcp,
        };
        assert_eq!(restricted.finding().score, 15);
    }

    #[test]
    fn exposes_ipv6_vnc_and_skips_connected_udp() {
        let mut bindings = BTreeSet::new();
        assert!(parse_table(
            "sl local_address rem_address st\n0: 00000000000000000000000000000000:170C 00000000000000000000000000000000:0000 0A\n",
            Transport::Tcp,
            true,
            &mut bindings
        ));
        let binding = bindings.first().unwrap();
        assert_eq!(binding.socket, "[::]:5900".parse().unwrap());
        assert_eq!(binding.finding().score, 75);
        assert!(parse_table(
            "sl local_address rem_address st\n0: 00000000:0035 00000000:0000 07\n1: 00000000:1000 00000000:007B 01\n",
            Transport::Udp,
            false,
            &mut bindings
        ));
        assert_eq!(bindings.len(), 2);
        assert!(!parse_table(
            "sl local_address rem_address st\n0: bad:170C 00000000:0000 0A\n",
            Transport::Tcp,
            false,
            &mut bindings
        ));
    }
}
