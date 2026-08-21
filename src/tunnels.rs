//! Port-forward tracking - the feature raw ssh can't give you. Nobody remembers
//! `-L port:host:port`, and once you background a tunnel with `-f` you lose the
//! PID and can't find or kill it later. So we spawn our own detached `ssh -N`,
//! record its PID + what it does in a tiny state file, and let you list/kill them.
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

/// A live (or once-live) forward. `spec` is the raw ssh forward argument, e.g.
/// `8080:localhost:80`; `kind` is 'L' (local) or 'R' (reverse/remote).
#[derive(Clone)]
pub struct Tunnel {
    pub pid: u32,
    pub kind: char,
    pub spec: String,
    pub host: String,
    /// File that captured this tunnel's stderr (for diagnosing failures).
    pub log: PathBuf,
}

impl Tunnel {
    /// One-line human summary for the listing / TUI.
    pub fn describe(&self) -> String {
        let arrow = if self.kind == 'L' {
            "local →"
        } else {
            "remote ←"
        };
        format!(
            "pid {:>7}  -{} {}  {} ({} {})",
            self.pid, self.kind, self.spec, self.host, arrow, self.host
        )
    }

    /// A tunnel is alive while its PID still has a `/proc` entry (Linux).
    pub fn alive(&self) -> bool {
        proc_alive(self.pid)
    }
}

fn proc_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

/// `~/.local/state/easyssh/` - persists tunnel state + logs across `essh` runs.
fn state_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("easyssh")
}

fn state_path() -> PathBuf {
    state_dir().join("tunnels.tsv")
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
    let _ = save(&live);
    live
}

fn save(tunnels: &[Tunnel]) -> Result<()> {
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

    // Give ssh a moment to connect; if it exited, report why from its stderr.
    sleep(Duration::from_millis(800));
    if !proc_alive(pid) {
        let reason = fs::read_to_string(&log).unwrap_or_default();
        let _ = fs::remove_file(&log);
        let first = reason
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("ssh exited immediately");
        bail!("{first}");
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
    save(&all)?;
    Ok(tunnel)
}

/// Terminate a tunnel by PID (SIGTERM via `kill`), delete its log, and forget it.
pub fn kill(pid: u32) -> Result<()> {
    let all = list();
    if let Some(t) = all.iter().find(|t| t.pid == pid) {
        let _ = fs::remove_file(&t.log);
    }
    let _ = Command::new("kill").arg(pid.to_string()).status();
    let remaining: Vec<Tunnel> = all.into_iter().filter(|t| t.pid != pid).collect();
    save(&remaining)?;
    Ok(())
}
