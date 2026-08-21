//! Drawing the frame: the tab bar, the list body, the status line and help.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs};

use super::confirm::render_confirm;
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
    render_body(f, chunks[1], app);
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
    let tabs = Tabs::new(vec!["Hosts", "Keys", "Tunnels", "Mounts (sshfs)"])
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

    match app.view {
        View::Hosts => {
            if app.hosts.is_empty() {
                empty(
                    f,
                    area,
                    "Hosts",
                    "No hosts in ~/.ssh/config yet.\nPress `n` to add one.",
                );
                return;
            }
            let w = app.hosts.iter().map(|h| h.alias.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = app
                .hosts
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:w$}", h.alias), bold),
                        Span::raw("  "),
                        Span::styled(h.target(), dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Hosts", app.hosts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.host_state);
        }

        View::Keys => {
            if app.keys.is_empty() {
                empty(
                    f,
                    area,
                    "Keys",
                    "No keypairs in ~/.ssh.\nPress `c` to generate one.",
                );
                return;
            }
            let nw = app.keys.iter().map(|k| k.name().len()).max().unwrap_or(0);
            let items: Vec<ListItem> = app
                .keys
                .iter()
                .map(|k| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:nw$}", k.name()), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:7}", k.kind), Style::default().fg(Color::Cyan)),
                        Span::raw("  "),
                        Span::styled(k.fingerprint.clone(), dim),
                        Span::raw("  "),
                        Span::styled(k.comment.clone(), dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Keys", app.keys.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.key_state);
        }

        View::Tunnels => {
            if app.tunnels.is_empty() {
                empty(
                    f,
                    area,
                    "Tunnels",
                    "No active tunnels.\nOpen one from the Hosts tab: `t` (forward) or `T` (expose).",
                );
                return;
            }
            let items: Vec<ListItem> = app
                .tunnels
                .iter()
                .map(|t| ListItem::new(Span::raw(t.describe())))
                .collect();
            let list = List::new(items)
                .block(titled("Tunnels", app.tunnels.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.tunnel_state);
        }

        View::Mounts => {
            if app.mounts.is_empty() {
                empty(
                    f,
                    area,
                    "Mounts",
                    "No sshfs mounts.\nMount one from the Hosts tab with `m`.",
                );
                return;
            }
            let items: Vec<ListItem> = app
                .mounts
                .iter()
                .map(|m| ListItem::new(Span::raw(m.describe())))
                .collect();
            let list = List::new(items)
                .block(titled("Mounts", app.mounts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.mount_state);
        }
    }
}

pub(super) fn render_status(f: &mut Frame, area: Rect, app: &App) {
    // Show the last action's result while it is fresh; otherwise the key hints,
    // so a stale message never masquerades as the current state.
    let (text, style) = match app.live_status() {
        Some(msg) => (msg, Style::default().fg(Color::Green)),
        None => {
            let hints = match app.view {
                View::Hosts => HOSTS_HINTS,
                View::Keys => KEYS_HINTS,
                View::Tunnels => TUNNELS_HINTS,
                View::Mounts => MOUNTS_HINTS,
            };
            (hints, Style::default().add_modifier(Modifier::DIM))
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))),
        area,
    );
}

pub(super) fn render_help(f: &mut Frame, area: Rect) {
    let rect = centered(area, 76, 25);
    f.render_widget(Clear, rect);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let lines = vec![
        Line::from(Span::styled(
            "easyssh - one tool for ssh, keys, tunnels, mounts",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("Move      j/k or ↑↓ list     h/l or ←→ switch view     (Ctrl+ works too)"),
        Line::raw("Global    ? help      q or Ctrl-c quit"),
        Line::raw(""),
        Line::raw("Hosts     ↵ connect (ssh <host>)      c new / e edit / d delete"),
        Line::raw("            n/e/d all write ~/.ssh/config for you"),
        Line::raw("          m mount a remote folder locally (sshfs host: ./dir)"),
        Line::raw("          t reach a remote port from here (ssh -L)"),
        Line::raw("          T expose a local port on the host (ssh -R)"),
        Line::raw("          R fix \"host key changed\" (ssh-keygen -R <host>)"),
        Line::raw(""),
        Line::raw("Keys      c new key (ssh-keygen -t ed25519)"),
        Line::raw("          y yank to a host (ssh-copy-id -i <key> <host>)"),
        Line::raw("Tunnels   d kill it (ends the background ssh -N)    r refresh"),
        Line::raw("Mounts    d unmount (fusermount -u <dir>)           r refresh"),
        Line::raw(""),
        Line::raw("In a form  type to fill (h/j/k/l are text!)"),
        Line::raw("           Ctrl-j/k · Ctrl-↑↓ · Tab move between fields · Esc cancel"),
        Line::raw("           on a ‹ choice › field h/l or ←/→ pick the answer"),
        Line::raw("In a yes/no  y confirm · n or Esc cancel · ←/→ then Enter"),
        Line::raw(""),
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
