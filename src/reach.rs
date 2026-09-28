//! Is that box actually up? A TCP connect to the host's ssh port, in the
//! background, so the Hosts list can say "up" or "down" before you press Enter
//! instead of leaving you staring at a connect that will time out.
//!
//! This is a reachability check, not an authentication one: a green dot means
//! something is listening on that port, which is exactly the question you have
//! when a connect hangs. Probes run off the UI thread and report back over a
//! channel; results carry the generation they were started in, so answers from
//! a superseded round are discarded rather than repainting a stale list.

use std::io::Read;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

/// How many hosts we probe at once. A config with a hundred entries should not
/// open a hundred sockets and a hundred threads at the same time.
const BATCH: usize = 8;

/// What we know about one host's ssh port right now.
#[derive(Clone, Copy, PartialEq)]
pub enum Reach {
    /// A probe is in flight.
    Probing,
    /// The port accepted a connection, in this many milliseconds.
    Up(u64),
    /// Refused, unroutable, unresolvable or slower than `TIMEOUT`.
    Down,
    /// Never asked: the host is reached through a `ProxyJump`, so its address is
    /// resolved on the jump host and a connect from here would be answering a
    /// question nobody asked. Only a login can tell you about one of these, and
    /// a login is not something a listing gets to do behind your back.
    Indirect,
}

/// One probe's answer, tagged with the round that asked for it.
pub struct Msg {
    pub alias: String,
    pub generation: u64,
    pub reach: Reach,
}

/// A host to probe: the alias we report under, and where it actually connects.
pub struct Target {
    pub alias: String,
    pub host: String,
    pub port: u16,
}

/// Probe every target in the background, `BATCH` at a time, sending each answer
/// as it lands. Returns immediately; the caller drains `tx`'s receiver.
pub fn probe_all(targets: Vec<Target>, generation: u64, tx: Sender<Msg>, timeout_secs: u64) {
    // Long enough for a box on the far side of a VPN, short enough that a dead
    // host resolves while you are still looking at the list. Yours to change in
    // Settings, because only you know which of those two you are.
    let timeout = Duration::from_secs(timeout_secs.clamp(1, 60));
    if targets.is_empty() {
        return;
    }
    thread::spawn(move || {
        for chunk in targets.chunks(BATCH) {
            // Scoped threads let the batch borrow `chunk` and guarantee they are
            // all finished before the next batch starts, which is what bounds
            // the concurrency without a pool.
            thread::scope(|s| {
                for t in chunk {
                    let tx = tx.clone();
                    s.spawn(move || {
                        let reach = probe(&t.host, t.port, timeout);
                        // A closed receiver just means the TUI is gone; there is
                        // nothing to report to and nothing to clean up.
                        let _ = tx.send(Msg {
                            alias: t.alias.clone(),
                            generation,
                            reach,
                        });
                    });
                }
            });
        }
    });
}

/// One TCP connect with a deadline, after resolving the name within the same
/// deadline.
fn probe(host: &str, port: u16, timeout: Duration) -> Reach {
    let started = Instant::now();
    // Try each address (a name can resolve to both A and AAAA records); the
    // first one that answers is the one ssh would have used.
    for ip in resolve(host, timeout) {
        if TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout).is_ok() {
            return Reach::Up(started.elapsed().as_millis() as u64);
        }
    }
    Reach::Down
}

/// The addresses `host` resolves to, in the resolver's order, or none when the
/// lookup fails or outlasts `timeout`.
///
/// The lookup runs in a `getent ahosts` child rather than through
/// `getaddrinfo` here: glibc's `exit` takes every stdio lock, and an NSS module
/// such as `mdns4_minimal` holds one while it waits on avahi, so a lookup still
/// in flight made quitting take as long as the lookup. `getent` asks the same
/// NSS chain ssh does, so the answers still match. Where `getent` cannot be
/// started, the name is resolved in-process.
fn resolve(host: &str, timeout: Duration) -> Vec<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    let Ok(mut child) = Command::new("getent")
        .args(["ahosts", host])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return (host, 0)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|a| a.ip()).collect())
            .unwrap_or_default();
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    parse_ahosts(&out)
}

/// The addresses in `getent ahosts` output, once each: it prints every address
/// three times, one line per socket type.
fn parse_ahosts(out: &str) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for ip in out
        .lines()
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
    {
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    ips
}
