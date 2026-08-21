//! Reading what ssh said when a connection failed, so the app can offer the fix.

use super::*;

/// Non-interactively check whether `host`'s key changed. Returns the exact target
/// to pass to `ssh-keygen -R` (parsed from ssh's own suggestion) when the key no
/// longer matches, or `None` for any other outcome (new host, auth failure, down).
/// Runs only after a failed connect, so it costs nothing on the success path.
pub(super) fn changed_host_key(host: &str) -> Option<String> {
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=6",
            "-o",
            "StrictHostKeyChecking=yes",
            host,
            "true",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    if !text.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return None;
    }
    // ssh prints "remove with: ssh-keygen -f '...' -R '<target>'"; use that exact
    // target, since it is the name/IP as stored in known_hosts, not our alias.
    Some(parse_r_target(&text).unwrap_or_else(|| host.to_string()))
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
