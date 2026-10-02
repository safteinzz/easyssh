//! Reading what ssh said when a connection failed, so the app can offer the fix.

use super::*;

/// Why a login failed, as far as ssh's own words tell.
#[derive(Debug, PartialEq)]
pub(super) enum Diagnosis {
    /// The host key no longer matches; `target` is what `ssh-keygen -R` takes.
    KeyChanged { target: String },
    /// The agent offered so many keys that the server gave up before the right one.
    TooManyKeys,
    /// The server takes keys only, and none of the ones offered is authorized.
    KeysOnly,
    /// Nothing listening on the port.
    Refused,
    /// The HostName does not resolve.
    Unresolved,
    /// No answer at all: off, another network, or a firewall.
    Unreachable,
}

/// Re-run a failed login non-interactively and classify it. Runs only after a
/// connect exited 255, so it costs nothing on the success path. Returns the
/// diagnosis and ssh's own lines, cut to what says what failed.
pub(super) fn diagnose(host: &str) -> Option<(Diagnosis, String)> {
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=6",
            "-o",
            "StrictHostKeyChecking=yes",
            // ssh refuses a command line on a host that forces one, before it
            // even connects, so the probe would never see the real failure.
            "-o",
            "RemoteCommand=none",
            host,
            "true",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    Some((classify(host, &text)?, said(&text)))
}

pub(super) fn classify(host: &str, text: &str) -> Option<Diagnosis> {
    if text.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        // ssh prints "remove with: ssh-keygen -f '...' -R '<target>'"; that
        // target is the name/IP as stored in known_hosts, not our alias.
        let target = parse_r_target(text).unwrap_or_else(|| host.to_string());
        return Some(Diagnosis::KeyChanged { target });
    }
    if text.contains("Too many authentication failures") {
        return Some(Diagnosis::TooManyKeys);
    }
    if let Some(rest) = text.split("Permission denied (").nth(1) {
        let methods = rest.split(')').next().unwrap_or("");
        // With a password method the interactive login asked for one, so a
        // wrong password is the likelier story and there is nothing to add.
        if !methods.contains("password") && !methods.contains("keyboard-interactive") {
            return Some(Diagnosis::KeysOnly);
        }
        return None;
    }
    if text.contains("Connection refused") {
        return Some(Diagnosis::Refused);
    }
    if text.contains("Could not resolve hostname") {
        return Some(Diagnosis::Unresolved);
    }
    if text.contains("timed out") || text.contains("No route to host") {
        return Some(Diagnosis::Unreachable);
    }
    None
}

/// ssh's stderr without the chatter, at most five lines in their order. The
/// lines that say what failed are kept first, since a server banner or
/// OpenSSH's post-quantum notice can come before them and fill the five.
pub(super) fn said(text: &str) -> String {
    const FAILED: [&str; 9] = [
        "Permission denied",
        "Connection refused",
        "Could not resolve",
        "timed out",
        "No route to host",
        "Too many authentication failures",
        "IDENTIFICATION HAS CHANGED",
        "Received disconnect",
        "Connection closed",
    ];
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("Warning: Permanently added")
                && !l.starts_with("Pseudo-terminal")
        })
        .collect();
    let failed = |l: &str| FAILED.iter().any(|f| l.contains(f));
    let mut keep: Vec<usize> = (0..lines.len())
        .filter(|&i| failed(lines[i]))
        .take(5)
        .collect();
    for i in 0..lines.len() {
        if keep.len() >= 5 {
            break;
        }
        if !keep.contains(&i) {
            keep.push(i);
        }
    }
    keep.sort_unstable();
    keep.iter()
        .map(|&i| lines[i])
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pull the value of `-R '<target>'` (or `-R <target>`) out of ssh's message.
pub(super) fn parse_r_target(text: &str) -> Option<String> {
    let after = text
        .split("-R ")
        .nth(1)?
        .trim_start()
        .trim_start_matches(['\'', '"']);
    let end = after
        .find(['\'', '"', ' ', '\n', '\r', '\t'])
        .unwrap_or(after.len());
    let target = after[..end].trim();
    (!target.is_empty()).then(|| target.to_string())
}

impl App {
    /// Turn a diagnosed login failure into the box that says what to do: an
    /// offer where the app can make the fix, an alert where only you can.
    pub(super) fn explain_connect_failure(&mut self, host: &str, d: Diagnosis, said: &str) {
        let entry = self.hosts.iter().find(|h| h.alias == host);
        let has_identity =
            entry.is_some_and(|h| h.identity.as_deref().is_some_and(|i| !i.is_empty()));
        let jump = entry
            .filter(|h| h.jumped())
            .and_then(|h| h.proxy_jump.clone());
        let cmd = format!("ssh {host}");
        // Behind a jump, a socket error may be the jump's or the target's, and
        // only ssh's own line below says which address it was.
        let via = |j: &str, what: &str| {
            format!(
                "{what} somewhere on the way to {host} through {j}: the address in ssh's line below says which machine."
            )
        };
        match d {
            Diagnosis::KeyChanged { target } => self.offer_known_hosts_fix(host, target),
            Diagnosis::TooManyKeys
                if has_identity && !sshcfg::sets_option(host, "IdentitiesOnly") =>
            {
                self.offer_identities_only(host)
            }
            Diagnosis::TooManyKeys if has_identity => self.alert(
                "too many keys offered",
                format!(
                    "{host} gave up before the right key was tried, although it already sets IdentitiesOnly: it has more IdentityFile or CertificateFile lines than the server allows attempts. `ssh -v {host}` lists what is offered.\n\n{cmd}\n\n{said}"
                ),
            ),
            Diagnosis::TooManyKeys => self.alert(
                "too many keys offered",
                format!(
                    "{host} gave up before the right key was tried, because your agent offered every key it holds first. Press `e` on {host} and set its IdentityFile (ctrl-o picks one); the next failure will offer to make ssh send only that key.\n\n{cmd}\n\n{said}"
                ),
            ),
            Diagnosis::KeysOnly => self.alert(
                "no key accepted",
                format!(
                    "{host} accepts keys only, and none of the ones ssh offered is in its authorized_keys. Check the User, or have someone who can log in add your public key (`y` on Keys copies it).\n\n{cmd}\n\n{said}"
                ),
            ),
            Diagnosis::Refused => {
                let why = match &jump {
                    Some(j) => via(j, "a connection was refused"),
                    None => format!(
                        "nothing is listening for ssh on {host}: sshd is stopped, or the Port is wrong (`e` edits it)."
                    ),
                };
                self.alert("connection refused", format!("{why}\n\n{cmd}\n\n{said}"))
            }
            Diagnosis::Unresolved => {
                let why = match &jump {
                    Some(j) => via(j, "a HostName does not resolve"),
                    None => format!(
                        "{host}'s HostName does not resolve: check it for a typo (`e` edits it)."
                    ),
                };
                self.alert("unknown host name", format!("{why}\n\n{cmd}\n\n{said}"))
            }
            Diagnosis::Unreachable => {
                let why = match &jump {
                    Some(j) => via(j, "a machine did not answer"),
                    None => format!(
                        "{host} did not answer: it is off, on another network, or a firewall drops ssh."
                    ),
                };
                self.alert("no answer", format!("{why}\n\n{cmd}\n\n{said}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_jump_is_read_as_refused() {
        let text = "ssh: connect to host 127.0.0.1 port 1: Connection refused\n\
                    Connection closed by UNKNOWN port 65535\n";
        assert_eq!(classify("target", text), Some(Diagnosis::Refused));
    }

    #[test]
    fn a_server_that_takes_passwords_is_not_called_keys_only() {
        let keys_only = "pi@10.0.0.6: Permission denied (publickey,gssapi-with-mic).\n";
        let with_password = "pi@10.0.0.6: Permission denied (publickey,password).\n";
        assert_eq!(classify("h", keys_only), Some(Diagnosis::KeysOnly));
        assert_eq!(
            classify("h", with_password),
            None,
            "a password login failing is a wrong password, not a missing key"
        );
    }

    #[test]
    fn a_changed_key_names_what_known_hosts_stores() {
        let text = "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\n\
                    remove with:\n  ssh-keygen -f '/home/u/.ssh/known_hosts' -R '[10.0.0.6]:2222'\n";
        assert_eq!(
            classify("raspi", text),
            Some(Diagnosis::KeyChanged {
                target: "[10.0.0.6]:2222".into()
            })
        );
    }

    #[test]
    fn too_many_keys_is_told_apart_from_a_plain_denial() {
        let text =
            "Received disconnect from 10.0.0.6 port 2222:2: Too many authentication failures\n";
        assert_eq!(classify("h", text), Some(Diagnosis::TooManyKeys));
    }

    #[test]
    fn the_failure_line_survives_a_long_banner() {
        let text = "welcome\nto\nthe\nbox\nplease\nbehave\npi@h: Permission denied (publickey).\n";
        assert!(
            said(text).contains("Permission denied"),
            "the line that says what failed must be among the five shown:\n{}",
            said(text)
        );
    }
}
