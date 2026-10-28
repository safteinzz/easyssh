//! Which hosts in the config are the machine essh is running on, so one
//! `~/.ssh/config` shared between machines does not list each of them to itself.
//!
//! Decided from this machine's own name and addresses, never a lookup: listing
//! hosts must not wait on DNS.

use crate::sshcfg::Host;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub struct ThisMachine {
    /// The kernel's hostname up to its first dot, lowercased; empty when it is
    /// `localhost`, which every machine answers to.
    name: String,
    /// The addresses on this machine's interfaces, loopback left out.
    addrs: Vec<IpAddr>,
}

impl ThisMachine {
    pub fn read() -> Self {
        let name = fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|n| first_label(n.trim()))
            .ok()
            .filter(|n| n != "localhost")
            .unwrap_or_default();
        Self {
            name,
            addrs: addresses(),
        }
    }

    /// Whether `ssh <alias>` logs in to this machine's own sshd: its alias or
    /// HostName is this machine's name, bare or under any domain, or one of its
    /// addresses. A jumped host is resolved on the jump, another port is another
    /// sshd (a container's, say) and loopback is as often a forward to somewhere
    /// else, so none of them count.
    pub fn is(&self, h: &Host) -> bool {
        if h.jumped() || h.port.as_deref().is_some_and(|p| p.trim() != "22") {
            return false;
        }
        [Some(h.alias.as_str()), h.hostname.as_deref()]
            .into_iter()
            .flatten()
            .any(|target| self.answers_to(target))
    }

    fn answers_to(&self, target: &str) -> bool {
        // An IPv6 HostName may carry its interface (`fe80::1%eth0`).
        let addr = target.split('%').next().unwrap_or(target);
        match addr.parse::<IpAddr>() {
            Ok(ip) => self.addrs.contains(&ip),
            Err(_) => !self.name.is_empty() && first_label(target) == self.name,
        }
    }
}

fn first_label(name: &str) -> String {
    name.split('.').next().unwrap_or("").to_ascii_lowercase()
}

/// IPv4 from the kernel's routing trie, where an address on an interface is a
/// `/32 host LOCAL` entry under its `|-- a.b.c.d` line; IPv6 from `if_inet6`,
/// which starts each line with one address as 32 hex digits.
fn addresses() -> Vec<IpAddr> {
    let mut addrs = Vec::new();
    let trie = fs::read_to_string("/proc/net/fib_trie").unwrap_or_default();
    let mut leaf = None;
    for line in trie.lines().map(str::trim) {
        if let Some(a) = line.strip_prefix("|-- ") {
            leaf = a.parse::<Ipv4Addr>().ok();
        } else if line == "/32 host LOCAL" {
            addrs.extend(leaf.map(IpAddr::V4));
        }
    }
    let inet6 = fs::read_to_string("/proc/net/if_inet6").unwrap_or_default();
    for line in inet6.lines() {
        let hex = line.split_whitespace().next().unwrap_or("");
        if let Ok(n) = u128::from_str_radix(hex, 16) {
            addrs.push(IpAddr::V6(Ipv6Addr::from(n)));
        }
    }
    addrs.retain(|a| !a.is_loopback());
    addrs.sort();
    addrs.dedup();
    addrs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(alias: &str, hostname: Option<&str>) -> Host {
        Host {
            alias: alias.into(),
            hostname: hostname.map(Into::into),
            user: None,
            port: None,
            identity: None,
            proxy_jump: None,
            remote_command: None,
            forward_agent: None,
        }
    }

    #[test]
    fn a_host_is_this_machine_by_name_or_address_but_never_through_a_jump_or_another_port() {
        let here = ThisMachine {
            name: "workbench".into(),
            addrs: vec![
                "192.0.2.10".parse().unwrap(),
                "2001:db8::10".parse().unwrap(),
            ],
        };
        assert!(
            here.is(&host("workbench", None)),
            "the alias is the hostname"
        );
        assert!(
            here.is(&host("wb", Some("WorkBench.example.com"))),
            "the HostName is the hostname under a domain, in any case"
        );
        assert!(
            here.is(&host("wb", Some("192.0.2.10"))),
            "a local IPv4 address"
        );
        assert!(
            here.is(&host("wb", Some("2001:db8::10%eth0"))),
            "a local IPv6 address with its interface"
        );
        assert!(
            !here.is(&host("other", Some("198.51.100.7"))),
            "another machine"
        );

        let mut jumped = host("workbench", None);
        jumped.proxy_jump = Some("bastion".into());
        assert!(!here.is(&jumped), "a jumped name is resolved on the jump");

        let mut container = host("wb", Some("192.0.2.10"));
        container.port = Some("2222".into());
        assert!(!here.is(&container), "another port is another sshd");
        container.port = Some("22".into());
        assert!(here.is(&container), "port 22 is the default sshd");
    }
}
