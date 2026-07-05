//! Port-forward tracking — the feature raw ssh can't give you. Nobody remembers
//! `-L port:host:port`, and once you background a tunnel with `-f` you lose the
//! PID and can't find or kill it later. So we spawn our own detached `ssh -N`,
//! record its PID + what it does in a tiny state file, and let you list/kill them.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A live (or once-live) forward. `spec` is the raw ssh forward argument, e.g.
/// `8080:localhost:80`; `kind` is 'L' (local) or 'R' (reverse/remote).
#[derive(Clone)]
pub struct Tunnel {
    pub pid: u32,
    pub kind: char,
    pub spec: String,
    pub host: String,
}

impl Tunnel {
    /// One-line human summary for the listing / TUI.
    pub fn describe(&self) -> String {
        let arrow = if self.kind == 'L' { "local →" } else { "remote ←" };
        format!("pid {:>7}  -{} {}  {} ({arrow} {})", self.pid, self.kind, self.spec, self.host, self.host)
    }

    /// A tunnel is alive while its PID still has a `/proc` entry (Linux).
    pub fn alive(&self) -> bool {
        PathBuf::from(format!("/proc/{}", self.pid)).exists()
    }
}

/// `~/.local/state/easyssh/tunnels.tsv` — survives across `essh` invocations
/// so `essh tunnel ls` works even after the launching process is gone.
fn state_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("easyssh").join("tunnels.tsv")
}

/// Read all tracked tunnels, dropping any whose process has died, and rewrite
/// the state file so it self-prunes. TSV: `pid\tkind\tspec\thost`.
pub fn list() -> Vec<Tunnel> {
    let path = state_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    let mut live = Vec::new();
    for line in text.lines() {
        let mut f = line.split('\t');
        let (Some(pid), Some(kind), Some(spec), Some(host)) = (f.next(), f.next(), f.next(), f.next()) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else { continue };
        let t = Tunnel {
            pid,
            kind: kind.chars().next().unwrap_or('L'),
            spec: spec.to_string(),
            host: host.to_string(),
        };
        if t.alive() {
            live.push(t);
        }
    }

    // Persist the pruned set (best effort — a stale line just reappears next run).
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
        .map(|t| format!("{}\t{}\t{}\t{}\n", t.pid, t.kind, t.spec, t.host))
        .collect();
    fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Open a forward: spawn `ssh -N -{L|R} <spec> <host>` fully detached (its own
/// process group, stdio to /dev/null) so it outlives us, then record it.
/// Assumes key/agent auth — a background tunnel can't answer a password prompt.
pub fn open(kind: char, spec: &str, host: &str) -> Result<Tunnel> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-N")
        .arg(format!("-{kind}"))
        .arg(spec)
        .arg(host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Detach from our controlling terminal's job control so Ctrl-C / our exit
    // doesn't take the tunnel down with us.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn().with_context(|| "spawning ssh tunnel")?;
    let tunnel = Tunnel {
        pid: child.id(),
        kind,
        spec: spec.to_string(),
        host: host.to_string(),
    };

    let mut all = list();
    all.push(tunnel.clone());
    save(&all)?;
    Ok(tunnel)
}

/// Terminate a tunnel by PID (SIGTERM via `kill`) and forget it.
pub fn kill(pid: u32) -> Result<()> {
    let _ = Command::new("kill").arg(pid.to_string()).status();
    let remaining: Vec<Tunnel> = list().into_iter().filter(|t| t.pid != pid).collect();
    save(&remaining)?;
    Ok(())
}
