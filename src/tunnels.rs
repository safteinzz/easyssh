//! Port-forward tracking - the feature raw ssh can't give you. Nobody remembers
//! `-L port:host:port`, and once you background a tunnel with `-f` you lose the
//! PID and can't find or kill it later. So we spawn our own detached `ssh -N`,
//! record its PID + what it does in a tiny state file, and let you list/kill them.
//!
//! A forward you use every day is worth more than a process, so every one you
//! open is kept: a line in `~/.config/easyssh/tunnels` that outlives the
//! process, the session and the reboot, and is turned on and off from the list.
//!
//! What identifies one is what it does - the flag, the spec and the host - so
//! that triple is the line's key and nothing has to be named to be listed. A
//! name is an optional label over the top of it, for saying *why* a forward
//! exists, which its ports cannot.
//!
//! So there are two files. `tunnels` (config) is what you keep; `tunnels.tsv`
//! (state) is what is running right now, pruned of anything that has died.
//! `Entry` is the two of them joined: the row you see.
//!
//! We capture each tunnel's stderr to its own log and, after spawning, wait a
//! beat to confirm it actually stayed up - so a bad port or auth failure surfaces
//! as an error instead of a "tunnel" that was already dead on arrival.

use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A live forward: the process carrying it, and what it was opened with.
#[derive(Clone)]
pub struct Tunnel {
    pub pid: u32,
    pub kind: char,
    pub spec: String,
    pub host: String,
    /// File that captured this tunnel's stderr (for diagnosing failures).
    pub log: PathBuf,
}

/// A forward you kept: a definition with no process behind it until you turn it
/// on. One `-L 5432:localhost:5432 vps` line in the config file each, plus
/// ` = a label` where you gave it one, so it can be read, written and diffed by
/// hand like everything else easyssh owns.
#[derive(Clone)]
pub struct Saved {
    pub kind: char,
    pub spec: String,
    pub host: String,
    /// What you called it, if anything. The row falls back to `Entry::label`.
    pub name: Option<String>,
}

/// One row of the Tunnels tab: a forward you keep, on or off.
pub struct Entry {
    pub name: Option<String>,
    pub kind: char,
    pub spec: String,
    pub host: String,
    /// The process carrying it, when it is up.
    pub live: Option<Tunnel>,
}

impl Saved {
    /// Is this the line for that forward? The flag, the spec and the host are
    /// what a forward is, so they are what identifies its line.
    pub fn matches(&self, kind: char, spec: &str, host: &str) -> bool {
        self.kind == kind && self.spec == spec && self.host == host
    }
}

impl Tunnel {
    /// When it was opened: the log file is created as the tunnel is spawned, so
    /// its mtime is the tunnel's birth time. `None` if the file has gone.
    pub fn started_at(&self) -> Option<SystemTime> {
        fs::metadata(&self.log).ok()?.modified().ok()
    }

    /// Whatever ssh wrote to stderr and kept running through - a warning about
    /// a port already in use, say. Empty for a forward that is simply working.
    pub fn stderr(&self) -> String {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// A tunnel is alive while its PID is still running *this* forward. The
    /// argv is checked rather than only `/proc/<pid>`: a saved tunnel's pid can
    /// outlive a reboot in the state file, and a recycled one would otherwise
    /// read as "up" and be what `d` kills.
    pub fn alive(&self) -> bool {
        proc_is_ours(self.pid, self.kind, &self.spec, &self.host)
    }
}

impl Entry {
    /// What the row is called. A typed name wins; otherwise it is spelled out
    /// from what the forward does, which is the whole of what one is - so
    /// nothing has to be named to be readable. `-L` reads as the service and
    /// then the port that answers for it here, `-R` the other way round.
    pub fn label(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let Some((open, target, port)) = self.ports() else {
            return format!("-{} {}", self.kind, self.spec);
        };
        // A bare `localhost` in the middle means whichever machine resolves it,
        // which is the far side for `-L` and this one for `-R`.
        let named_target = !matches!(target, "localhost" | "127.0.0.1");
        match self.kind {
            'L' if named_target => format!("{target}:{port} → {open}"),
            'L' => format!("{}:{port} → {open}", self.host),
            _ if named_target => format!("{target}:{port} → {}:{open}", self.host),
            _ => format!("{port} → {}:{open}", self.host),
        }
    }

    /// Is it up?
    pub fn on(&self) -> bool {
        self.live.is_some()
    }

    pub fn pid(&self) -> Option<u32> {
        self.live.as_ref().map(|t| t.pid)
    }

    /// One-line summary: what it is called, what it forwards, whether it is up.
    /// This is also what `/` filters on, so everything on the row is in it.
    pub fn describe(&self) -> String {
        format!(
            "{}  -{} {} {} ({})",
            self.label(),
            self.kind,
            self.spec,
            self.host,
            if self.on() { "on" } else { "off" }
        )
    }

    /// The three parts of a `-L`/`-R` spec: the port opened on this side, and
    /// the `host:port` the far side connects onward to. `None` for a spec we
    /// did not write (a bind address in front makes it four fields).
    pub fn ports(&self) -> Option<(&str, &str, &str)> {
        let mut f = self.spec.split(':');
        match (f.next(), f.next(), f.next(), f.next()) {
            (Some(open), Some(target), Some(port), None) => Some((open, target, port)),
            _ => None,
        }
    }

    /// What this forward does, in the words of what you would use it for.
    pub fn explain(&self) -> String {
        let Some((open, target, port)) = self.ports() else {
            return format!("-{} {}", self.kind, self.spec);
        };
        // The middle field is resolved on the *far* side, so a bare `localhost`
        // there means the server itself, not this machine.
        let local_target = matches!(target, "localhost" | "127.0.0.1");
        match (self.kind, local_target) {
            ('L', true) => format!("localhost:{open} here is {}'s own {port}", self.host),
            ('L', false) => format!(
                "localhost:{open} here is {target}:{port}, reached from {}",
                self.host
            ),
            (_, true) => format!("{}:{open} there is this machine's {port}", self.host),
            (_, false) => format!(
                "{}:{open} there is {target}:{port}, reached from here",
                self.host
            ),
        }
    }

    /// The command that opens it, exactly as it is run.
    pub fn command(&self) -> String {
        format!("ssh -N -{} {} {}", self.kind, self.spec, self.host)
    }
}

/// Is this pid running the forward we think it is? `/proc/<pid>` existing only
/// says *something* is running under that number.
fn proc_is_ours(pid: u32, kind: char, spec: &str, host: &str) -> bool {
    let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let flag = format!("-{kind}");
    args.contains(&flag) && args.iter().any(|a| a == spec) && args.iter().any(|a| a == host)
}

/// `~/.local/state/easyssh/` - what is running, and the logs. Wiped by a reboot
/// without losing anything, because nothing you chose is kept here.
fn state_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("easyssh")
}

fn state_path() -> PathBuf {
    state_dir().join("tunnels.tsv")
}

/// `~/.config/easyssh/tunnels` - the forwards you named, beside `settings`.
pub fn saved_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("easyssh")
        .join("tunnels")
}

fn log_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    state_dir().join("tunnels").join(format!("{stamp}.log"))
}

/// Read all tracked tunnels, dropping any whose process has died, and rewrite
/// the state file so it self-prunes. TSV: `pid\tkind\tspec\thost\tlog`.
pub fn list() -> Vec<Tunnel> {
    let Ok(text) = fs::read_to_string(state_path()) else {
        return Vec::new();
    };

    let mut live = Vec::new();
    for line in text.lines() {
        let mut f = line.split('\t');
        let (Some(pid), Some(kind), Some(spec), Some(host)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        // `log` is optional so a state file from an older build still parses.
        let log = f.next().map(PathBuf::from).unwrap_or_default();
        let t = Tunnel {
            pid,
            kind: kind.chars().next().unwrap_or('L'),
            spec: spec.to_string(),
            host: host.to_string(),
            log,
        };
        if t.alive() {
            live.push(t);
        } else if !t.log.as_os_str().is_empty() {
            let _ = fs::remove_file(&t.log); // clean the log of a tunnel that has died
        }
    }

    // Persist the pruned set (best effort - a stale line just reappears next run).
    let _ = save_state(&live);
    live
}

fn save_state(tunnels: &[Tunnel]) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body: String = tunnels
        .iter()
        .map(|t| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                t.pid,
                t.kind,
                t.spec,
                t.host,
                t.log.display()
            )
        })
        .collect();
    fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The rows of the Tunnels tab: everything you keep, in the order the file has
/// it, each carrying the process running it when there is one.
///
/// Anything running that has no line of its own is written into the file first.
/// A forward that is up is one you have, so "running" and "kept" can never
/// disagree - and a tunnel opened by an older build, or one whose line was
/// deleted by hand while it ran, is adopted rather than left as a row that
/// could be turned off and never back on.
pub fn entries() -> Vec<Entry> {
    let live = list();
    for t in &live {
        if !saved().iter().any(|d| d.matches(t.kind, &t.spec, &t.host)) {
            let _ = remember(&Saved {
                kind: t.kind,
                spec: t.spec.clone(),
                host: t.host.clone(),
                name: None,
            });
        }
    }

    saved()
        .into_iter()
        .map(|def| {
            // Same flag, same spec, same host is the same forward: that is all
            // one is, and it is what the line is keyed by.
            let running = live
                .iter()
                .find(|t| def.matches(t.kind, &t.spec, &t.host))
                .cloned();
            Entry {
                name: def.name,
                kind: def.kind,
                spec: def.spec,
                host: def.host,
                live: running,
            }
        })
        .collect()
}

/// `-L 5432:localhost:5432 vps`, optionally ` = a label`: the shape a saved
/// forward is written in. What it does comes first because that is what
/// identifies it; the label is yours and may be anything, `=` included. A line
/// that is not one of these is skipped rather than refused, since this file is
/// meant to be edited by hand and one bad line must not cost you the others.
fn parse_saved(line: &str) -> Option<Saved> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (forward, name) = match line.split_once('=') {
        Some((forward, name)) => (
            forward,
            Some(name.trim().to_string()).filter(|n| !n.is_empty()),
        ),
        None => (line, None),
    };
    let mut f = forward.split_whitespace();
    let kind = match f.next()? {
        "-L" | "L" => 'L',
        "-R" | "R" => 'R',
        _ => return None,
    };
    Some(Saved {
        kind,
        spec: f.next()?.to_string(),
        host: f.next()?.to_string(),
        name,
    })
}

fn saved_line(def: &Saved) -> String {
    let forward = format!("-{} {} {}", def.kind, def.spec, def.host);
    match &def.name {
        Some(name) => format!("{forward} = {name}"),
        None => forward,
    }
}

/// Every forward you named, in the order the file has them - which is the order
/// they were added, until you reorder the file yourself.
pub fn saved() -> Vec<Saved> {
    saved_from(&fs::read_to_string(saved_path()).unwrap_or_default())
}

fn saved_from(text: &str) -> Vec<Saved> {
    text.lines().filter_map(parse_saved).collect()
}

const SAVED_HEADER: &str = "\
# easyssh tunnels - the forwards you keep, one `-L spec host` line each, and
# ` = a label` after it where you named one. -L reaches a remote service from
# here, -R exposes a local one over there. The Tunnels tab writes this file;
# `enter` turns a line on and off.

";

/// Add a saved forward, or replace the line that already describes it. Every
/// other line survives untouched, comments included: it is a file you are meant
/// to be able to edit.
pub fn remember(def: &Saved) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    write_saved(&path, &with_saved(&text, def))
}

/// Drop a saved forward. Whether it is running is the caller's business: the
/// file only says what you keep.
pub fn forget(kind: char, spec: &str, host: &str) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    write_saved(&path, &without_saved(&text, kind, spec, host))
}

/// The file with this definition in it: replacing the line that already has
/// that name, or appended if it is new. Every other line is copied through as
/// it was written.
fn with_saved(text: &str, def: &Saved) -> String {
    let mut body = if text.trim().is_empty() {
        SAVED_HEADER.to_string()
    } else {
        String::new()
    };
    let mut replaced = false;
    for line in text.lines() {
        match parse_saved(line) {
            Some(s) if s.matches(def.kind, &def.spec, &def.host) => {
                if !replaced {
                    body.push_str(&saved_line(def));
                    body.push('\n');
                    replaced = true;
                }
            }
            _ => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    if !replaced {
        body.push_str(&saved_line(def));
        body.push('\n');
    }
    body
}

/// The file without that forward's line, and otherwise unchanged.
fn without_saved(text: &str, kind: char, spec: &str, host: &str) -> String {
    text.lines()
        .filter(|l| !matches!(parse_saved(l), Some(s) if s.matches(kind, spec, host)))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn write_saved(path: &std::path::Path, body: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(path, body).with_context(|| format!("writing {}", path.display()))
}

/// Open a forward: spawn `ssh -N -{L|R} <spec> <host>` fully detached (its own
/// process group, stderr to a log file) so it outlives us, wait ~1s to confirm
/// it connected, then record it. If ssh bailed (bad port, auth, DNS) the captured
/// stderr becomes the returned error instead of a phantom tunnel.
/// Assumes key/agent auth - a background tunnel can't answer a password prompt.
pub fn open(kind: char, spec: &str, host: &str) -> Result<Tunnel> {
    let log = log_path();
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent)?;
    }
    let errfile = File::create(&log).with_context(|| format!("creating {}", log.display()))?;

    let mut cmd = Command::new("ssh");
    cmd.arg("-N")
        .arg(format!("-{kind}"))
        .arg(spec)
        .arg(host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(errfile));

    // Detach from our controlling terminal's job control so Ctrl-C / our exit
    // doesn't take the tunnel down with us.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn().with_context(|| "spawning ssh tunnel")?;
    let pid = child.id();

    // Give ssh a moment to connect, then judge it by what it wrote rather than
    // only by whether it is still running.
    sleep(Duration::from_millis(800));
    let stderr = fs::read_to_string(&log).unwrap_or_default();
    let first = || {
        stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("ssh exited immediately")
            .to_string()
    };
    if !proc_is_ours(pid, kind, spec, host) {
        let reason = first();
        let _ = fs::remove_file(&log);
        bail!("{reason}");
    }
    // A forward whose port is already taken does NOT kill ssh: it keeps the
    // session open with no forward on it, which used to be listed as a live
    // tunnel that quietly carried nothing. Take ssh at its word and clean up.
    if forwarding_failed(&stderr) {
        let reason = first();
        let _ = Command::new("kill").arg(pid.to_string()).status();
        let _ = fs::remove_file(&log);
        bail!("{reason}");
    }

    let tunnel = Tunnel {
        pid,
        kind,
        spec: spec.to_string(),
        host: host.to_string(),
        log,
    };

    let mut all = list();
    all.push(tunnel.clone());
    save_state(&all)?;
    Ok(tunnel)
}

/// Did ssh tell us the forward itself could not be set up? These are the exact
/// phrases OpenSSH uses; the session survives all of them, so nothing else
/// would notice.
fn forwarding_failed(stderr: &str) -> bool {
    const FAILURES: [&str; 4] = [
        "Address already in use",
        "cannot listen to port",
        "Could not request local forwarding",
        "remote port forwarding failed",
    ];
    FAILURES.iter().any(|f| stderr.contains(f))
}

/// Terminate a tunnel by PID (SIGTERM via `kill`), delete its log, and forget it
/// is running. A saved forward keeps its definition: this is the off switch, not
/// the delete key.
pub fn kill(pid: u32) -> Result<()> {
    let all = list();
    if let Some(t) = all.iter().find(|t| t.pid == pid) {
        let _ = fs::remove_file(&t.log);
    }
    let _ = Command::new("kill").arg(pid.to_string()).status();
    let remaining: Vec<Tunnel> = all.into_iter().filter(|t| t.pid != pid).collect();
    save_state(&remaining)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: char, spec: &str) -> Entry {
        Entry {
            name: None,
            kind,
            spec: spec.to_string(),
            host: "raspi".into(),
            live: None,
        }
    }

    #[test]
    fn a_forward_explains_which_side_is_which() {
        // The middle field of a spec is resolved on the far side, so a bare
        // `localhost` there is the server, not this machine. Getting that
        // backwards is the single most confusing thing about -L and -R.
        let l = entry('L', "8080:localhost:80");
        assert_eq!(l.explain(), "localhost:8080 here is raspi's own 80");

        let r = entry('R', "9000:localhost:3000");
        assert_eq!(r.explain(), "raspi:9000 there is this machine's 3000");

        // A third host in the middle is neither end of the ssh session.
        let via = entry('L', "5432:db.internal:5432");
        assert_eq!(
            via.explain(),
            "localhost:5432 here is db.internal:5432, reached from raspi"
        );
    }

    #[test]
    fn a_forward_says_what_it_does_when_nothing_named_it() {
        // The README promises a row reads `raspi:80 → 8080` without your having
        // to name it, so this is what the list falls back to for every forward
        // that was never labelled.
        assert_eq!(entry('L', "8080:localhost:80").label(), "raspi:80 → 8080");
        assert_eq!(
            entry('R', "9000:localhost:3000").label(),
            "3000 → raspi:9000"
        );

        // A third host in the middle is the one being reached, on whichever
        // side resolves it, so it is the one the label names.
        assert_eq!(
            entry('L', "5432:db.internal:5432").label(),
            "db.internal:5432 → 5432"
        );
        assert_eq!(
            entry('R', "9000:printer.lan:631").label(),
            "printer.lan:631 → raspi:9000"
        );

        // A name, when there is one, is the whole point of having typed it.
        let mut named = entry('L', "8080:localhost:80");
        named.name = Some("pihole".into());
        assert_eq!(named.label(), "pihole");
    }

    #[test]
    fn the_saved_file_survives_a_round_trip_and_a_hand_edit() {
        // The file is a README promise, so what we write has to parse back, and a
        // hand-written line has to survive us writing to it.
        let file = "# mine\n-L 8080:localhost:80 raspi = pihole\n-R 5432:db.lan:5432 vps\n";
        let defs = saved_from(file);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name.as_deref(), Some("pihole"));
        assert_eq!(
            (defs[1].kind, defs[1].spec.as_str()),
            ('R', "5432:db.lan:5432")
        );
        assert!(defs[1].name.is_none());

        // The line is keyed by the forward, so relabelling one rewrites it
        // where it stands and leaves every other line, comment included, as it
        // was.
        let relabelled = Saved {
            name: Some("dns".into()),
            ..defs[0].clone()
        };
        assert_eq!(
            with_saved(file, &relabelled),
            "# mine\n-L 8080:localhost:80 raspi = dns\n-R 5432:db.lan:5432 vps\n"
        );

        // A different forward is a different line, appended: moving one is the
        // caller forgetting the old key, which is what editing does.
        let moved = Saved {
            spec: "9090:localhost:80".into(),
            ..defs[0].clone()
        };
        assert_eq!(saved_from(&with_saved(file, &moved)).len(), 3);
        assert_eq!(
            saved_from(&without_saved(
                &with_saved(file, &moved),
                'L',
                &defs[0].spec,
                "raspi"
            ))
            .len(),
            2
        );

        // Forgetting takes its line and nothing else.
        assert_eq!(
            without_saved(file, 'L', "8080:localhost:80", "raspi"),
            "# mine\n-R 5432:db.lan:5432 vps\n"
        );
        assert_eq!(without_saved(file, 'L', "nothing:like:this", "raspi"), file);

        // A file we did not write is skipped line by line rather than refused.
        assert!(saved_from("nonsense\n-Q 1:2:3 host\n= just a label\n").is_empty());
    }

    #[test]
    fn the_command_is_the_one_that_ran() {
        assert_eq!(
            entry('L', "8080:localhost:80").command(),
            "ssh -N -L 8080:localhost:80 raspi"
        );
    }

    #[test]
    fn a_spec_we_did_not_write_is_left_alone() {
        // A bind address in front makes four fields; rather than mis-explain it,
        // fall back to showing the flag as given.
        let odd = entry('L', "127.0.0.1:8080:localhost:80");
        assert!(odd.ports().is_none());
        assert_eq!(odd.explain(), "-L 127.0.0.1:8080:localhost:80");
    }

    #[test]
    fn ssh_saying_it_could_not_forward_is_a_failure() {
        // ssh keeps the session open when a forward is refused, so the process
        // being alive proves nothing. These are its own words for the failure.
        assert!(forwarding_failed(
            "bind [127.0.0.1]:5432: Address already in use"
        ));
        assert!(forwarding_failed(
            "channel_setup_fwd_listener_tcpip: cannot listen to port: 5432"
        ));
        assert!(forwarding_failed("Could not request local forwarding."));
        assert!(forwarding_failed(
            "Warning: remote port forwarding failed for listen port 9000"
        ));

        // A working tunnel says nothing, and a banner is not a failure.
        assert!(!forwarding_failed(""));
        assert!(!forwarding_failed(
            "Warning: Permanently added 'raspi' (ED25519)"
        ));
    }
}
