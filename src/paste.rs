//! Pasting an image from this machine's clipboard into Claude Code running on a
//! host you are connected to.
//!
//! Claude Code on Linux reads a pasted image by running `xclip`, which has no
//! clipboard to read on a headless box. A host you enable gets a stand-in
//! `~/.local/bin/xclip` (`paste/xclip.sh`), and every `essh` login to it serves
//! this machine's clipboard on a local unix socket that `ssh -R` forwards into
//! `~/.cache/easyssh/paste/` over there, for exactly as long as the login lasts.
//!
//! Enabled hosts are kept in `~/.config/easyssh/paste`, one `alias remote-dir`
//! line each. The remote directory is absolute because ssh rejects a relative
//! `-R` socket path and would expand a `~` on this side, and it is learned from
//! the installer because only the remote knows its own home.
//!
//! sshd, not ssh, decides whether a stale socket file is replaced
//! (`StreamLocalBindUnlink` in sshd_config, default no), and it never removes
//! one when a login ends. Every login therefore binds a new name, and the
//! stand-in deletes the ones nothing listens on any more.

use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::PathBuf;

/// The stand-in installed as `~/.local/bin/xclip` on an enabled host.
const STANDIN: &str = include_str!("paste/xclip.sh");
const INSTALL: &str = include_str!("paste/install.sh");
const REMOVE: &str = include_str!("paste/remove.sh");
/// The line that marks a remote `xclip` as ours. Line 2 of `paste/xclip.sh`.
const MARKER: &str = "essh image-paste stand-in";

/// Longest socket path we hand to `ssh -R`: `sun_path` holds 104 bytes on macOS
/// and 108 on Linux, and ssh refuses the whole login over a path that does not
/// fit (`Bad remote forwarding specification`), not just the forward.
const MAX_SOCKET_PATH: usize = 100;
/// The longest name a login gives its remote socket, `<epoch>-<pid>.sock`.
const SOCKET_NAME_MAX: usize = 32;

/// `~/.config/easyssh/paste`, beside `tunnels` and `mounts`.
pub fn saved_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("easyssh")
        .join("paste")
}

/// A host that gets image paste: its alias and the absolute directory its
/// forwarded sockets go in.
#[derive(Clone, Debug, PartialEq)]
pub struct Enabled {
    pub alias: String,
    pub dir: String,
}

fn parse_line(line: &str) -> Option<Enabled> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let mut f = line.split_whitespace();
    let (alias, dir) = (f.next()?, f.next()?);
    (f.next().is_none() && dir.starts_with('/')).then(|| Enabled {
        alias: alias.into(),
        dir: dir.into(),
    })
}

/// Every enabled host, in file order. A line that is not `alias /dir` is
/// skipped, since the file is yours to edit.
pub fn enabled() -> Vec<Enabled> {
    fs::read_to_string(saved_path())
        .unwrap_or_default()
        .lines()
        .filter_map(parse_line)
        .collect()
}

/// The enabled entry for exactly this destination, if any.
pub fn enabled_for(alias: &str) -> Option<Enabled> {
    enabled().into_iter().find(|e| e.alias == alias)
}

const SAVED_HEADER: &str = "\
# easyssh image paste: the hosts that may read the image on your clipboard
# while you are connected with essh, one `alias remote-socket-dir` line each.
# `P` on a host in essh adds or removes a line.

";

/// Enable `alias`, or update the directory of a host already enabled. Every
/// other line is kept as written.
pub fn remember(alias: &str, dir: &str) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut body = if text.trim().is_empty() {
        SAVED_HEADER.to_string()
    } else {
        String::new()
    };
    for line in text.lines() {
        if !matches!(parse_line(line), Some(e) if e.alias == alias) {
            body.push_str(line);
            body.push('\n');
        }
    }
    body.push_str(&format!("{alias} {dir}\n"));
    write_saved(&path, &body)
}

/// Stop forwarding the clipboard to `alias`. What is installed over there is
/// the caller's business.
pub fn forget(alias: &str) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    let body: String = text
        .lines()
        .filter(|l| !matches!(parse_line(l), Some(e) if e.alias == alias))
        .map(|l| format!("{l}\n"))
        .collect();
    write_saved(&path, &body)
}

fn write_saved(path: &std::path::Path, body: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(path, body).with_context(|| format!("writing {}", path.display()))
}

/// `ssh <alias>` running `script` under `sh`, whatever the login shell is.
///
/// The script travels base64-encoded on the command line rather than on stdin,
/// so stdin stays the terminal and ssh can still ask for a password, and so no
/// login shell (fish and csh quote differently) ever parses it.
fn remote_sh(alias: &str, script: &str) -> Vec<String> {
    let mut argv: Vec<String> = [
        "ssh",
        // ssh refuses a command line on a host that forces one.
        "-o",
        "RemoteCommand=none",
        "-o",
        "RequestTTY=no",
        // The host's own forwards would be bound a second time, beside a login
        // that may already hold them.
        "-o",
        "ClearAllForwardings=yes",
    ]
    .map(String::from)
    .to_vec();
    argv.push(alias.into());
    argv.push(format!(
        "echo {} | base64 -d | sh",
        base64(script.as_bytes())
    ));
    argv
}

/// The command that installs the stand-in on `alias`.
pub fn install_argv(alias: &str) -> Vec<String> {
    let script = INSTALL
        .replace("@MARKER@", MARKER)
        .replace("@STANDIN@", STANDIN.trim_end());
    remote_sh(alias, &script)
}

/// The command that removes the stand-in from `alias`, if it is ours.
pub fn remove_argv(alias: &str) -> Vec<String> {
    remote_sh(alias, &REMOVE.replace("@MARKER@", MARKER))
}

/// The `essh-paste:` lines the remote scripts print, without the prefix.
fn reports(stdout: &str) -> impl Iterator<Item = &str> {
    stdout
        .lines()
        .filter_map(|l| l.trim_end().strip_prefix("essh-paste: "))
}

/// What the installer found, once it has written the stand-in.
#[derive(Debug, PartialEq)]
pub struct Installed {
    /// Where this host's sockets go, absolute.
    pub dir: String,
    /// What would keep paste from working although the install succeeded.
    pub warnings: Vec<String>,
}

/// Read the installer's report. `Err` says, in words for the user, why
/// nothing was installed.
pub fn read_install(alias: &str, stdout: &str) -> Result<Installed, String> {
    let (mut dir, mut resolves, mut bin, mut helper) = (None, None, None, None);
    for r in reports(stdout) {
        let (key, value) = r.split_once(' ').unwrap_or((r, ""));
        match key {
            "not-linux" => {
                return Err(format!(
                    "{alias} runs {value}, and only Claude Code on Linux reads the clipboard through xclip, so nothing was installed."
                ));
            }
            "no-home" => return Err(format!("{alias} has no $HOME, so nothing was installed.")),
            "taken" => {
                return Err(format!(
                    "{alias} already has a ~/.local/bin/xclip that essh did not write, so it was left alone and nothing was installed. Move it aside there and press P again."
                ));
            }
            "dir" => dir = Some(value.to_string()),
            "resolves" => resolves = Some(value.to_string()),
            "bin" => bin = Some(value.to_string()),
            "helper" => helper = Some(value.to_string()),
            _ => {}
        }
    }
    let dir = dir.ok_or_else(|| format!("{alias} did not report where the sockets go."))?;
    if dir.contains([':', ' ', '\t']) || dir.len() + 1 + SOCKET_NAME_MAX > MAX_SOCKET_PATH {
        return Err(format!(
            "`{dir}` on {alias} is too long or holds a `:` or a space, which `ssh -R` cannot carry, so image paste stays off."
        ));
    }
    let mut warnings = Vec::new();
    match (resolves.as_deref(), bin.as_deref()) {
        (Some(r), Some(b)) if r == b => {}
        (Some(""), _) | (None, _) => warnings.push(format!(
            "A login shell on {alias} did not find the stand-in, so ~/.local/bin is probably not on its PATH. Add it before /usr/bin in ~/.profile there and log in again."
        )),
        (Some(r), _) => warnings.push(format!(
            "A login shell on {alias} runs {r}, not the stand-in, so put ~/.local/bin before /usr/bin in PATH there (in ~/.profile) and log in again."
        )),
    }
    if helper.as_deref() == Some("none") {
        warnings.push(format!(
            "{alias} has none of python3, perl, socat or a `nc` with -U, so the stand-in cannot reach the socket. Install one of them there."
        ));
    }
    Ok(Installed { dir, warnings })
}

/// What the remover found.
#[derive(Debug, PartialEq)]
pub enum Removal {
    Done,
    /// There was no `~/.local/bin/xclip` to remove.
    Absent,
    /// The `~/.local/bin/xclip` there is not ours, so it stayed.
    NotOurs,
}

pub fn read_remove(stdout: &str) -> Option<Removal> {
    reports(stdout).find_map(|r| match r {
        "removed" => Some(Removal::Done),
        "absent" => Some(Removal::Absent),
        "not-ours" => Some(Removal::NotOurs),
        _ => None,
    })
}

/// Standard base64 with padding, which `base64 -d` on the remote reads.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | ((b as u32) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(unix)]
pub use server::{Forward, serve_for};

#[cfg(not(unix))]
pub struct Forward;

/// Unix sockets are what the forward is made of, so there is nothing to serve
/// here.
#[cfg(not(unix))]
pub fn serve_for(_alias: &str) -> Result<Option<Forward>> {
    Ok(None)
}

#[cfg(not(unix))]
impl Forward {
    pub fn insert_into(&self, _argv: &mut Vec<String>, _at: usize) {}
}

#[cfg(unix)]
mod server {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// How long one clipboard read may take before it counts as no image: an
    /// X11 owner that stopped answering keeps `xclip -o` waiting forever.
    const READ_TIMEOUT: Duration = Duration::from_secs(5);

    /// The local readers, first that hands back a PNG wins: Wayland, X11, macOS.
    const READERS: [(&str, &[&str]); 3] = [
        ("wl-paste", &["--no-newline", "--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
        ("pngpaste", &["-"]),
    ];

    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    /// The clipboard served for one login. Dropping it stops the server and
    /// removes the socket, so keep it alive until ssh has exited.
    pub struct Forward {
        local: PathBuf,
        remote: String,
        stop: Arc<AtomicBool>,
    }

    /// Start serving the clipboard when `dest` is an enabled host, exactly as
    /// typed: an alias logs in as one user, and the stand-in is installed for
    /// that one.
    pub fn serve_for(dest: &str) -> Result<Option<Forward>> {
        let Some(enabled) = enabled_for(dest) else {
            return Ok(None);
        };
        let pid = std::process::id();
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let remote = format!("{}/{epoch}-{pid}.sock", enabled.dir.trim_end_matches('/'));
        let local = socket_dir()?.join(format!("paste-{pid}.sock"));
        let shown = local.to_string_lossy();
        if shown.contains(':') || shown.len() > MAX_SOCKET_PATH {
            return Err(anyhow!(
                "`{shown}` is too long or holds a `:`, which `ssh -R` cannot carry; set XDG_RUNTIME_DIR to a short path"
            ));
        }
        if remote.len() > MAX_SOCKET_PATH {
            return Err(anyhow!(
                "`{remote}` is too long for `ssh -R`; shorten {dest}'s line in `{}`",
                crate::sshcfg::collapse_tilde(&saved_path().to_string_lossy())
            ));
        }
        // A file left by an earlier essh that had this pid; the directory is ours alone.
        let _ = fs::remove_file(&local);
        let listener = UnixListener::bind(&local)
            .map_err(|e| anyhow!("could not listen on `{shown}`: {}", e.kind()))?;
        fs::set_permissions(&local, fs::Permissions::from_mode(0o600))
            .map_err(|e| anyhow!("could not make `{shown}` private: {}", e.kind()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || serve(listener, flag));
        Ok(Some(Forward {
            local,
            remote,
            stop,
        }))
    }

    impl Forward {
        /// Put `-R remote:local` into an ssh argv, before the destination at `at`.
        pub fn insert_into(&self, argv: &mut Vec<String>, at: usize) {
            let spec = format!("{}:{}", self.remote, self.local.to_string_lossy());
            argv.splice(at..at, ["-R".to_string(), spec]);
        }
    }

    impl Drop for Forward {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // `accept` only returns for a connection, so make one to let the
            // thread see `stop`, while the socket still exists.
            let _ = UnixStream::connect(&self.local);
            let _ = fs::remove_file(&self.local);
        }
    }

    /// `$XDG_RUNTIME_DIR/easyssh`, or the easyssh state dir where there is none,
    /// made private to this user.
    fn socket_dir() -> Result<PathBuf> {
        let dir = match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
            Some(run) => PathBuf::from(run).join("easyssh"),
            None => dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .ok_or_else(|| anyhow!("no runtime or state directory; set XDG_RUNTIME_DIR"))?
                .join("easyssh")
                .join("paste"),
        };
        let shown = dir.display();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| anyhow!("could not create `{shown}`: {}", e.kind()))?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| anyhow!("could not make `{shown}` private: {}", e.kind()))?;
        Ok(dir)
    }

    fn serve(listener: UnixListener, stop: Arc<AtomicBool>) {
        for conn in listener.incoming() {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            match conn {
                Ok(mut conn) => {
                    let _ = answer(&mut conn);
                }
                // Out of descriptors, most likely: do not spin on it.
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    }

    /// One request, one line (`png` or `targets`), answered and closed. No
    /// image is an empty answer, which the stand-in reads as "ask xclip".
    fn answer(conn: &mut UnixStream) -> std::io::Result<()> {
        conn.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut request = Vec::new();
        BufReader::new(conn.try_clone()?.take(32)).read_until(b'\n', &mut request)?;
        let reply = match String::from_utf8_lossy(&request).trim() {
            "png" => read_png(),
            "targets" => read_png().map(|_| b"TARGETS\nimage/png\n".to_vec()),
            _ => None,
        };
        if let Some(reply) = reply {
            conn.write_all(&reply)?;
        }
        Ok(())
    }

    /// The clipboard as PNG bytes, or `None` when it holds no image or no
    /// reader is installed.
    fn read_png() -> Option<Vec<u8>> {
        READERS
            .iter()
            .filter(|(cmd, _)| on_path(cmd))
            .find_map(|(cmd, args)| run(cmd, args).filter(|b| b.starts_with(PNG_MAGIC)))
    }

    /// A reader's stdout, or `None` if it failed or outlived `READ_TIMEOUT`.
    /// Its stdin and stderr are closed, because ssh owns the terminal.
    fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        // Read while waiting, since a screenshot is bigger than the pipe and
        // the reader blocks until someone drains it.
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let deadline = Instant::now() + READ_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let bytes = reader.join().ok()?.ok()?;
        (status.success() && !bytes.is_empty()).then_some(bytes)
    }

    fn on_path(program: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
            .unwrap_or(false)
    }
}
