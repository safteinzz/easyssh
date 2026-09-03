//! The panel beside the list: everything known about the selected row, so the
//! answers you would otherwise go looking for (is it up, when was I last on it,
//! which key does it use, is that key even in my agent) are on screen already.
//!
//! It only appears when the terminal is wide enough to keep a readable list
//! next to it; below that the list gets the whole width.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::*;

/// Narrowest total width that still leaves a usable list beside the panel.
pub(super) const MIN_WIDTH_FOR_DETAIL: u16 = 96;
/// The panel's share of the frame when it is shown.
pub(super) const DETAIL_PCT: u16 = 38;

/// Does this view have a detail panel, and is there room for it? Every view
/// does; the width is what decides.
pub(super) fn detail_fits(_app: &App, width: u16) -> bool {
    width >= MIN_WIDTH_FOR_DETAIL
}

pub(super) fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let lines = match app.view {
        View::Hosts => host_lines(app),
        View::Keys => key_lines(app),
        View::Tunnels => tunnel_lines(app),
        View::Mounts => mount_lines(app),
        View::Settings => setting_lines(app),
    };
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Details ")
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn host_lines(app: &App) -> Vec<Line<'static>> {
    let Some(h) = app.selected_host() else {
        return vec![Line::styled("nothing selected", dim())];
    };
    let mut lines = vec![Line::styled(h.alias.clone(), heading()), Line::raw("")];

    // State first: the two things that decide whether you press Enter at all.
    lines.push(reach_line(app.reach.get(&h.alias), h.proxy_jump.as_deref()));
    lines.push(match app.history.get(&h.alias) {
        Some(e) => Line::styled(
            format!(
                "last {} · {} connect{}",
                history::ago(e.last, history::now()),
                e.count,
                if e.count == 1 { "" } else { "s" }
            ),
            dim(),
        ),
        None => Line::styled("never connected from here", dim()),
    });
    lines.push(Line::raw(""));

    for (label, value) in [
        ("HostName", h.hostname.clone()),
        ("User", h.user.clone()),
        ("Port", h.port.clone()),
        ("Key", h.identity.clone()),
        ("Via", h.proxy_jump.clone()),
        ("Runs", h.remote_command.clone()),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            lines.push(row(label, v));
        }
    }

    // What this host is currently doing, pulled from the other two tabs so you
    // do not have to go and look.
    let tunnels: Vec<String> = app
        .tunnels
        .iter()
        .filter(|t| t.host == h.alias)
        .map(|t| format!("-{} {}", t.kind, t.spec))
        .collect();
    let mounts: Vec<String> = app
        .mounts
        .iter()
        .filter(|m| {
            // `/proc/mounts` shows the sshfs source as `[user@]host:path`; the
            // host part is what we mounted, which is this alias.
            let src = m.remote.split(':').next().unwrap_or("");
            src.rsplit('@').next().unwrap_or(src) == h.alias
        })
        .map(|m| m.local.clone())
        .collect();
    if !tunnels.is_empty() || !mounts.is_empty() {
        lines.push(Line::raw(""));
        if !tunnels.is_empty() {
            lines.push(row("Tunnels", tunnels.join(", ")));
        }
        if !mounts.is_empty() {
            lines.push(row("Mounts", mounts.join(", ")));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("ssh {}", h.alias),
        Style::default().fg(Color::Green),
    ));
    lines
}

fn key_lines(app: &App) -> Vec<Line<'static>> {
    let Some(k) = app.selected_key() else {
        return vec![Line::styled("nothing selected", dim())];
    };
    let mut lines = vec![Line::styled(k.name(), heading()), Line::raw("")];

    let size = if k.bits.is_empty() {
        k.kind.clone()
    } else {
        format!("{} {} bits", k.kind, k.bits)
    };
    lines.push(row("Type", size));
    // The fingerprint gets its own line: `SHA256:` already says what it is, and
    // a label column in front of it would only push it into a ragged wrap.
    if !k.fingerprint.is_empty() {
        lines.push(Line::styled(k.fingerprint.clone(), dim()));
    }
    if !k.comment.is_empty() {
        lines.push(row("Comment", k.comment.clone()));
    }
    lines.push(row("Path", tilde(&k.path)));
    lines.push(Line::raw(""));

    // The two facts that decide whether a connect with this key is silent or
    // asks you for something.
    lines.push(Line::from(vec![
        label("Agent"),
        match k.agent_loaded {
            true => Span::styled("loaded (ssh-add -l)", Style::default().fg(Color::Green)),
            false => Span::styled("not loaded (ssh-add <key>)", dim()),
        },
    ]));
    lines.push(Line::from(vec![
        label("Unlock"),
        match k.encrypted {
            true => Span::styled(
                "passphrase on every use",
                Style::default().fg(Color::Yellow),
            ),
            false => Span::styled("none: the file is the secret", dim()),
        },
    ]));

    lines.push(Line::raw(""));
    lines.push(match k.used_by.is_empty() {
        true => row("Used by", "no host pins it (IdentityFile)".into()),
        false => row("Used by", k.used_by.join(", ")),
    });
    lines
}

fn tunnel_lines(app: &App) -> Vec<Line<'static>> {
    let Some(t) = app.selected_tunnel() else {
        return vec![Line::styled("nothing selected", dim())];
    };
    let mut lines = vec![
        Line::styled(format!("-{} {}", t.kind, t.spec), heading()),
        Line::raw(""),
    ];

    // What it is for, before what it is made of.
    lines.push(Line::raw(t.explain()));
    lines.push(Line::raw(""));

    if let Some(reach) = app.reach.get(&t.host) {
        lines.push(reach_line(Some(reach), jump_of(app, &t.host)));
    }
    lines.push(row("Host", t.host.clone()));
    lines.push(row("Process", format!("pid {}", t.pid)));
    if let Some(started) = t.started_at() {
        // The log file is created as the tunnel is spawned, so its age is the
        // tunnel's age.
        let secs = started
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        lines.push(row("Opened", history::ago(secs, history::now())));
    }

    // A forward that half-died is otherwise silent: ssh's own words are the
    // only explanation there is.
    let stderr = t.stderr();
    if !stderr.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(stderr, Style::default().fg(Color::Yellow)));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(t.command(), Style::default().fg(Color::Green)));
    lines.push(Line::styled(format!("kill {}", t.pid), dim()));
    lines
}

fn mount_lines(app: &App) -> Vec<Line<'static>> {
    let Some(m) = app.selected_mount() else {
        return vec![Line::styled("nothing selected", dim())];
    };
    let mut lines = vec![Line::styled(shorten(&m.local), heading()), Line::raw("")];

    // A mount is a live ssh session, so the state of the host behind it is the
    // first thing that explains a directory that has gone unresponsive. We only
    // know it for a host that is also in the config and has been probed.
    if let Some(reach) = app.reach.get(m.host()) {
        lines.push(reach_line(Some(reach), jump_of(app, m.host())));
        lines.push(Line::raw(""));
    }

    lines.push(row("Host", m.host().to_string()));
    if let Some(user) = m.user() {
        lines.push(row("As", user.to_string()));
    }
    lines.push(row("Remote", m.remote_path().to_string()));
    lines.push(row("Local", shorten(&m.local)));
    lines.push(row(
        "Access",
        match m.has_option("ro") {
            true => "read-only".to_string(),
            false => "read and write".to_string(),
        },
    ));
    // Who owns the files on this side. sshfs maps every remote file to one local
    // uid/gid, which is why a mount can look like it is owned by you when the
    // remote thinks otherwise.
    if let (Some(uid), gid) = (m.option("user_id"), m.option("group_id")) {
        lines.push(row(
            "Shown as",
            match gid {
                Some(gid) => format!("uid {uid} · gid {gid} (locally)"),
                None => format!("uid {uid} (locally)"),
            },
        ));
    }
    let others = m.other_options();
    if !others.is_empty() {
        lines.push(row("Flags", others.join(", ")));
    }

    lines.push(Line::raw(""));
    // Shown home-relative like every other path here: this line is for a human
    // with a shell, which expands `~` itself. The spawned unmount still gets the
    // absolute `m.local`, because it runs without a shell.
    lines.push(Line::styled(
        format!("fusermount -u {}", shorten(&m.local)),
        Style::default().fg(Color::Green),
    ));
    lines
}

fn setting_lines(app: &App) -> Vec<Line<'static>> {
    let Some(r) = app.selected_setting() else {
        return vec![Line::styled("nothing selected", dim())];
    };
    let mut lines = vec![Line::styled(r.label.to_string(), heading()), Line::raw("")];
    lines.push(Line::styled(r.help.to_string(), dim()));
    lines.push(Line::raw(""));
    lines.push(row("Now", r.value.clone()));
    lines.push(row("Default", r.default.clone()));
    if let Some(choices) = r.choices {
        lines.push(row("Choices", choices.join(" · ")));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        match r.choices {
            Some(_) => "↵ cycles it · d puts it back".to_string(),
            None => "↵ types it · d puts it back".to_string(),
        },
        dim(),
    ));
    lines.push(Line::raw(""));
    // The file is the real store; say where it is so it can be edited or
    // version-controlled like anything else.
    lines.push(Line::styled(
        shorten(&settings::path().to_string_lossy()),
        dim(),
    ));
    lines.push(row("Key", format!("{} = {}", r.key, r.value)));
    lines
}

/// One `label   value` row, with the label dim so the value reads first.
fn row(name: &str, value: String) -> Line<'static> {
    Line::from(vec![label(name), Span::raw(value)])
}

/// The dim, fixed-width label column that keeps every value aligned.
fn label(name: &str) -> Span<'static> {
    Span::styled(format!("{name:<8} "), dim())
}

/// The jump an alias goes through, for a reach line that has only the name.
fn jump_of<'a>(app: &'a App, alias: &str) -> Option<&'a str> {
    let h = app.hosts.iter().find(|h| h.alias == alias)?;
    h.proxy_jump.as_deref().filter(|_| h.jumped())
}

/// What we know about getting to a host. With a jump, every word is about the
/// jump: it is the only end of the route this machine can ask about, and a line
/// that said "up" would be claiming an answer nobody has.
fn reach_line(reach: Option<&Reach>, via: Option<&str>) -> Line<'static> {
    if let Some(via) = via.filter(|v| !v.trim().is_empty() && v.trim() != "none") {
        let via = via.trim().to_string();
        return match reach {
            Some(Reach::Up(_)) => Line::styled(
                format!("◆ the jump {via} answered · this host is not checked from here"),
                Style::default().fg(Color::Green),
            ),
            Some(Reach::Down) => Line::styled(
                format!("◆ the jump {via} is down · nothing here can reach this host"),
                Style::default().fg(Color::Red),
            ),
            Some(Reach::Probing) => Line::styled(format!("○ checking the jump {via}…"), dim()),
            _ => Line::styled(
                format!("· reached through {via} · not checked from here"),
                dim(),
            ),
        };
    }
    match reach {
        // A loopback or LAN answer rounds to zero, and "answered in 0 ms" reads
        // like a missing number rather than a fast one.
        Some(Reach::Up(0)) => Line::styled(
            "● up · answered instantly".to_string(),
            Style::default().fg(Color::Green),
        ),
        Some(Reach::Up(ms)) => Line::styled(
            format!("● up · answered in {ms} ms"),
            Style::default().fg(Color::Green),
        ),
        Some(Reach::Down) => Line::styled(
            "● down · nothing listening on the ssh port",
            Style::default().fg(Color::Red),
        ),
        Some(Reach::Indirect) => Line::styled("· not checked from here", dim()),
        Some(Reach::Probing) => Line::styled("○ checking the ssh port…", dim()),
        None => Line::styled("○ not checked (r to check)", dim()),
    }
}

/// `/home/you/.ssh/id_rsa` -> `~/.ssh/id_rsa`, which is how you wrote it.
fn tilde(path: &std::path::Path) -> String {
    shorten(&path.to_string_lossy())
}

/// The same shortening for a path we already hold as text.
fn shorten(path: &str) -> String {
    sshcfg::collapse_tilde(path)
}

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn heading() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}
