//! Drawing the frame: the tab bar, the list body, the detail panel, the status
//! line and help.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs};

use super::confirm::render_confirm;
use super::detail::{DETAIL_PCT, detail_fits};
use super::picker::render_picker;
use super::*;

pub(super) fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(f, chunks[0], app);
    // The list keeps the whole width until there is room for a panel that does
    // not squeeze it; below that threshold the row itself has to say everything.
    if detail_fits(app, area.width) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(100 - DETAIL_PCT),
                Constraint::Percentage(DETAIL_PCT),
            ])
            .split(chunks[1]);
        render_body(f, cols[0], app);
        render_detail(f, cols[1], app);
    } else {
        render_body(f, chunks[1], app);
    }
    render_status(f, chunks[2], app);

    if app.show_help {
        render_help(f, area);
    }
    if let Some(p) = &app.prompt {
        render_prompt(f, area, p);
    }
    if let Some(p) = &app.picker {
        render_picker(f, area, p);
    }
    if let Some(c) = &app.confirm {
        render_confirm(f, area, c);
    }
}

pub(super) fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let idx = VIEWS.iter().position(|v| *v == app.view).unwrap_or(0);
    let tabs = Tabs::new(vec![
        "Hosts",
        "Keys",
        "Tunnels",
        "Mounts (sshfs)",
        "Settings",
    ])
    .select(idx)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" easyssh · essh "),
    )
    .divider("│")
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(tabs, area);
}

pub(super) fn render_body(f: &mut Frame, area: Rect, app: &mut App) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let sel = Style::default().add_modifier(Modifier::REVERSED);
    let now = history::now();

    match app.view {
        View::Hosts => {
            let rows = app.host_rows();
            if let Some(msg) = nothing_here(
                app,
                app.hosts.len(),
                rows.len(),
                "No hosts in ~/.ssh/config yet.\nPress `c` to add one.",
            ) {
                empty(f, area, "Hosts", &msg);
                return;
            }
            // Columns are sized to what is actually on screen, so a filtered
            // list tightens up instead of keeping a hidden host's width.
            let w = rows
                .iter()
                .map(|&i| app.hosts[i].alias.len())
                .max()
                .unwrap_or(0);
            let tw = rows
                .iter()
                .map(|&i| app.hosts[i].target().len())
                .max()
                .unwrap_or(0)
                .min(34);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| {
                    let h = &app.hosts[i];
                    let (mark, style) = reach_mark(app.reach.get(&h.alias));
                    let age = match app.history.get(&h.alias) {
                        Some(e) => history::ago(e.last, now),
                        None => String::new(),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, style),
                        Span::raw(" "),
                        Span::styled(format!("{:w$}", h.alias), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:tw$}", h.target()), dim),
                        Span::raw("  "),
                        Span::styled(age, dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(counted("Hosts", rows.len(), app.hosts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.host_state);
        }

        View::Keys => {
            let rows = app.key_rows();
            if let Some(msg) = nothing_here(
                app,
                app.keys.len(),
                rows.len(),
                "No keypairs in ~/.ssh.\nPress `c` to generate one.",
            ) {
                empty(f, area, "Keys", &msg);
                return;
            }
            let nw = rows
                .iter()
                .map(|&i| app.keys[i].name().len())
                .max()
                .unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| {
                    let k = &app.keys[i];
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:nw$}", k.name()), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:7}", k.kind), Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{:>5}", k.bits), dim),
                        Span::raw("  "),
                        // The two states worth scanning for: a key in the agent
                        // connects silently, an encrypted one will ask.
                        Span::styled(
                            format!("{:6}", if k.agent_loaded { "agent" } else { "" }),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled(
                            format!("{:11}", if k.encrypted { "passphrase" } else { "" }),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(k.fingerprint.clone(), dim),
                        Span::raw("  "),
                        Span::styled(k.comment.clone(), dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(counted("Keys", rows.len(), app.keys.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.key_state);
        }

        View::Tunnels => {
            let rows = app.tunnel_rows();
            if let Some(msg) = nothing_here(
                app,
                app.tunnels.len(),
                rows.len(),
                "No active tunnels.\nOpen one from the Hosts tab: `t` (forward) or `T` (expose).",
            ) {
                empty(f, area, "Tunnels", &msg);
                return;
            }
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| ListItem::new(Span::raw(app.tunnels[i].describe())))
                .collect();
            let list = List::new(items)
                .block(counted("Tunnels", rows.len(), app.tunnels.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.tunnel_state);
        }

        View::Mounts => {
            let rows = app.mount_rows();
            if let Some(msg) = nothing_here(
                app,
                app.mounts.len(),
                rows.len(),
                "No sshfs mounts.\nMount one from the Hosts tab with `m`.",
            ) {
                empty(f, area, "Mounts", &msg);
                return;
            }
            let items: Vec<ListItem> = rows
                .iter()
                .map(|&i| ListItem::new(Span::raw(app.mounts[i].describe())))
                .collect();
            let list = List::new(items)
                .block(counted("Mounts", rows.len(), app.mounts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.mount_state);
        }

        View::Settings => {
            let rows = app.settings_rows();
            let total = app.settings.rows().len();
            if let Some(msg) = nothing_here(app, total, rows.len(), "No settings.") {
                empty(f, area, "Settings", &msg);
                return;
            }
            let w = rows.iter().map(|r| r.label.len()).max().unwrap_or(0);
            // Sized to the widest value on screen: a long path must not run
            // into the help text beside it.
            let vw = rows.iter().map(|r| r.value.len()).max().unwrap_or(0);
            let gw = rows
                .iter()
                .map(|r| r.group.label().len())
                .max()
                .unwrap_or(0);
            let mut last_group = None;
            let items: Vec<ListItem> = rows
                .iter()
                .map(|r| {
                    // The group is named once, on its first row, so the two
                    // kinds read as two blocks without a header you can land on.
                    let group = if last_group == Some(r.group) {
                        String::new()
                    } else {
                        r.group.label().to_string()
                    };
                    last_group = Some(r.group);
                    // A value you chose is worth picking out from one that just
                    // came with the program.
                    let value_style = if r.is_default() {
                        Style::default()
                    } else {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{group:gw$}"), dim),
                        Span::raw("  "),
                        Span::styled(format!("{:w$}", r.label), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:vw$}", r.value), value_style),
                        Span::raw("  "),
                        Span::styled(r.help, dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(counted("Settings", rows.len(), total))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.settings_state);
        }
    }
}

/// The message for an empty pane, or `None` when there are rows to draw. An
/// empty list and a filter that matched nothing are different problems, so they
/// get different words.
fn nothing_here(app: &App, total: usize, shown: usize, when_empty: &str) -> Option<String> {
    if total == 0 {
        Some(when_empty.to_string())
    } else if shown == 0 {
        Some(format!(
            "Nothing matches '{}'.\nEsc clears the filter.",
            app.query
        ))
    } else {
        None
    }
}

/// `Name (shown/total)` while a filter is on, plain `Name (n)` otherwise.
fn counted(name: &str, shown: usize, total: usize) -> Block<'static> {
    if shown == total {
        titled(name, total)
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {name} ({shown}/{total}) "))
    }
}

/// The dot in front of a host: filled and coloured once we know, hollow while
/// we are still asking.
fn reach_mark(reach: Option<&Reach>) -> (&'static str, Style) {
    match reach {
        Some(Reach::Up(_)) => ("●", Style::default().fg(Color::Green)),
        Some(Reach::Down) => ("●", Style::default().fg(Color::Red)),
        _ => ("○", Style::default().add_modifier(Modifier::DIM)),
    }
}

pub(super) fn render_status(f: &mut Frame, area: Rect, app: &App) {
    // While `/` is being typed the line belongs to the query: it is the only
    // place what you typed is visible.
    if app.searching {
        let spans = vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.query.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("█"),
            Span::styled(
                format!("   {} match   ↵ keep · Esc clear", app.row_count()),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ];
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    // Show the last action's result while it is fresh; otherwise the key hints,
    // so a stale message never masquerades as the current state.
    let (text, style) = match app.live_status() {
        Some(msg) => (msg.to_string(), Style::default().fg(Color::Green)),
        None => {
            let hints = match app.view {
                View::Hosts => HOSTS_HINTS,
                View::Keys => KEYS_HINTS,
                View::Tunnels => TUNNELS_HINTS,
                View::Mounts => MOUNTS_HINTS,
                View::Settings => SETTINGS_HINTS,
            };
            // A committed filter stays visible in front of the hints: rows are
            // hidden, and nothing else on screen would say why.
            let text = if app.query.is_empty() {
                hints.to_string()
            } else {
                format!("/{}  (Esc clears) · {hints}", app.query)
            };
            (text, Style::default().add_modifier(Modifier::DIM))
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))),
        area,
    );
}

pub(super) fn render_help(f: &mut Frame, area: Rect) {
    let rect = centered(area, 78, 30);
    f.render_widget(Clear, rect);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let lines = vec![
        Line::from(Span::styled(
            "easyssh - one tool for ssh, keys, tunnels, mounts",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("Move      j/k or ↑↓ list     h/l or ←→ switch view     (Ctrl+ works too)"),
        Line::raw("Global    / filter this list   ? help   q or Ctrl-c quit"),
        Line::raw(""),
        Line::raw("Hosts     ↵ connect (ssh <host>)      c new / e edit / d delete"),
        Line::raw("          y yank `ssh <host>` to the clipboard"),
        Line::raw("          m mount a remote folder locally (sshfs host: ./dir)"),
        Line::raw("          t reach a remote port from here (ssh -L)"),
        Line::raw("          T expose a local port on the host (ssh -R)"),
        Line::raw("          R fix \"host key changed\" (ssh-keygen -R <host>)"),
        Line::raw("          r reload · ● up · ● down · ○ checking the ssh port"),
        Line::raw(""),
        Line::raw("Keys      c new key (ssh-keygen -t ed25519)"),
        Line::raw("          y yank to a host (ssh-copy-id -i <key> <host>)"),
        Line::raw("          Y yank the public key to the clipboard"),
        Line::raw("          agent = loaded in ssh-agent · passphrase = asks to unlock"),
        Line::raw("Tunnels   d kill it (ends the background ssh -N)    r refresh"),
        Line::raw("Mounts    d unmount (fusermount -u <dir>)           r refresh"),
        Line::raw("Settings  ↵ change it · d back to default · r reload the file"),
        Line::raw(""),
        Line::raw("In a form  type to fill (h/j/k/l are text!)"),
        Line::raw("           Ctrl-j/k · Ctrl-↑↓ · Tab move between fields · Esc cancel"),
        Line::raw("           Ctrl-o picks a key (IdentityFile) or a host (ProxyJump)"),
        Line::raw("           paste user@host:port in Alias and it fills the rest"),
        Line::raw("           on a ‹ choice › field h/l or ←/→ pick the answer"),
        Line::raw("In a yes/no  y confirm · n or Esc cancel · ←/→ then Enter"),
        Line::from(Span::styled("press ? or Esc to close", dim)),
    ];
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" help ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(para, rect);
}
