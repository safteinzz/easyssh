//! Drawing the frame: the tab bar, the list body, the detail panel, the status
//! line and help.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs};

use super::confirm::render_confirm;
use super::detail::{DETAIL_PCT, detail_fits};
use super::picker::render_picker;
use super::widgets::{READER_KEYS, SEP, vscrollbar};
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
        render_help(f, area, app);
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
    // Last, so a failure is never drawn under the thing that caused it.
    if let Some(a) = &app.alert {
        super::alert::render_alert(f, area, a);
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
                    let (mark, style) = reach_mark(app.reach.get(&h.alias), h.jumped());
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
            list_bar(f, area, rows.len(), app.host_state.offset());
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
                        Span::styled(k.kind.clone(), Style::default().fg(Color::Cyan)),
                        pad(&k.kind, 7),
                        Span::styled(format!("{:>5}", k.bits), dim),
                        Span::raw("  "),
                        // The two states worth scanning for: a key in the agent
                        // connects silently, an encrypted one will ask.
                        Span::styled(
                            if k.agent_loaded { "agent" } else { "" },
                            Style::default().fg(Color::Green),
                        ),
                        pad(if k.agent_loaded { "agent" } else { "" }, 6),
                        Span::styled(
                            if k.encrypted { "passphrase" } else { "" },
                            Style::default().fg(Color::Yellow),
                        ),
                        pad(if k.encrypted { "passphrase" } else { "" }, 11),
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
            list_bar(f, area, rows.len(), app.key_state.offset());
        }

        View::Tunnels => {
            let rows = app.tunnel_rows();
            if let Some(msg) = nothing_here(
                app,
                app.tunnels.len(),
                rows.len(),
                "No tunnels yet.\nPress `c` here, or `t` (reach) / `T` (expose) on a host.",
            ) {
                empty(f, area, "Tunnels", &msg);
                return;
            }
            // Labels as wide as the widest on screen, so the specs line up under
            // each other and a list of forwards reads as a table.
            let labels: Vec<String> = rows.iter().map(|&i| app.tunnels[i].label()).collect();
            let nw = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            let sw = rows
                .iter()
                .map(|&i| app.tunnels[i].spec.len())
                .max()
                .unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .zip(&labels)
                .map(|(&i, label)| {
                    let t = &app.tunnels[i];
                    // On or off in words, not a dot: the dots already mean
                    // "can this machine reach that port" on the Hosts tab, and
                    // a symbol that means two things means neither.
                    let (state, state_style) = match t.on() {
                        true => ("on ", Style::default().fg(Color::Green)),
                        false => ("off", dim),
                    };
                    // Padded on the label's *characters*, since a derived one
                    // carries an arrow and `{:width$}` counts bytes.
                    let pad = " ".repeat(nw.saturating_sub(label.chars().count()));
                    ListItem::new(Line::from(vec![
                        Span::styled(state, state_style),
                        Span::raw("  "),
                        Span::styled(label.clone(), bold),
                        Span::raw(pad),
                        Span::raw("  "),
                        Span::raw(format!("-{} ", t.kind)),
                        Span::raw(format!("{:sw$}", t.spec)),
                        Span::raw("  "),
                        Span::raw(t.host.clone()),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(counted("Tunnels", rows.len(), app.tunnels.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.tunnel_state);
            list_bar(f, area, rows.len(), app.tunnel_state.offset());
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
                .map(|&i| {
                    let m = &app.mounts[i];
                    let (state, state_style) = match m.on {
                        true => ("on ", Style::default().fg(Color::Green)),
                        false => ("off", dim),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(state, state_style),
                        Span::raw("  "),
                        Span::raw(m.describe()),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(counted("Mounts", rows.len(), app.mounts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.mount_state);
            list_bar(f, area, rows.len(), app.mount_state.offset());
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
            list_bar(f, area, rows.len(), app.settings_state.offset());
        }
    }
}

/// The scrollbar of a bordered list pane, after the list is drawn so its
/// `offset` is the one on screen.
fn list_bar(f: &mut Frame, area: Rect, total: usize, offset: usize) {
    vscrollbar(
        f,
        area,
        total,
        offset,
        area.height.saturating_sub(2) as usize,
    );
}

/// The message for an empty pane, or `None` when there are rows to draw. An
/// empty list and a filter that matched nothing are different problems, so they
/// get different words.
fn nothing_here(app: &App, total: usize, shown: usize, when_empty: &str) -> Option<String> {
    if total == 0 {
        Some(when_empty.to_string())
    } else if shown == 0 {
        Some(format!(
            "Nothing matches '{}'.\n`esc` clears the filter.",
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
/// we are still asking. A jumped host wears a diamond instead, because the
/// answer is about its first hop and not about the host itself - the colour is
/// the same question, asked one machine away. Filled, not hollow: a thin
/// outline at terminal sizes reads as white whatever colour it is drawn in,
/// and the colour is the whole point of the mark.
fn reach_mark(reach: Option<&Reach>, jumped: bool) -> (&'static str, Style) {
    let known = if jumped { "◆" } else { "●" };
    match reach {
        Some(Reach::Up(_)) => (known, Style::default().fg(Color::Green)),
        Some(Reach::Down) => (known, Style::default().fg(Color::Red)),
        // Nothing was asked, so nothing is claimed: a mark that is neither
        // filled nor the hollow one a probe in flight wears.
        Some(Reach::Indirect) => ("·", Style::default().add_modifier(Modifier::DIM)),
        _ => ("○", Style::default().add_modifier(Modifier::DIM)),
    }
}

pub(super) fn render_status(f: &mut Frame, area: Rect, app: &App) {
    // While `/` is being typed the line belongs to the query: it is the only
    // place what you typed is visible.
    if app.searching {
        let mut spans = vec![Span::styled(
            " /",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(line_edit::with_cursor(
            &app.query,
            app.query_back,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        // Every letter goes into the query here, so only keys that are not
        // letters are offered.
        spans.push(Span::styled(
            format!("   {} match   ↵ keep{SEP}{BACK}", app.row_count()),
            Style::default().add_modifier(Modifier::DIM),
        ));
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    // Show the last action's result while it is fresh; otherwise the keys, so a
    // stale message never masquerades as the current state.
    let line = match app.live_status() {
        Some(msg) => Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(if app.status_failed {
                Color::Yellow
            } else {
                Color::Green
            }),
        )),
        None => {
            let keys = match app.view {
                View::Hosts => HOSTS_KEYS,
                View::Keys => KEYS_KEYS,
                View::Tunnels => TUNNELS_KEYS,
                View::Mounts => MOUNTS_KEYS,
                View::Settings => SETTINGS_KEYS,
            };
            // A committed filter stays visible in front of the keys: rows are
            // hidden, and nothing else on screen would say why.
            let lead = match app.query.is_empty() {
                true => Vec::new(),
                false => vec![format!("/{}", app.query), BACK.to_string()],
            };
            key_footer(&lead, keys, area.width)
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/// One group of the help panel: a heading, then `(keys, what they do)` rows,
/// where a row with no keys is a note about the group.
type HelpSection = (&'static str, &'static [(&'static str, &'static str)]);

/// Every key the app answers to, grouped by where it works. The panel scrolls,
/// so a new row costs nothing but its line.
const HELP: &[HelpSection] = &[
    (
        "moving",
        &[
            ("j/k ↑↓", "move in the list"),
            ("h/l ←→", "the previous, next tab"),
            ("tab shift-tab", "the next, previous tab"),
            ("ctrl-j/k/h/l", "the same, from anywhere"),
        ],
    ),
    (
        "every tab",
        &[
            ("/", "find in the list, esc drops it"),
            ("r", "refresh what the tab shows"),
            ("?", "this help"),
            ("q ctrl-c", "quit (ctrl-c is esc in a box or a form)"),
        ],
    ),
    (
        "hosts",
        &[
            ("↵", "connect"),
            ("c", "create a host in ~/.ssh/config"),
            ("e", "edit it"),
            ("d", "delete it (the config is backed up first)"),
            ("m", "mount a remote folder locally (sshfs)"),
            ("t", "reach a remote port from here (ssh -L)"),
            ("T", "expose a local port on the host (ssh -R)"),
            ("P", "on/off: paste images into Claude Code there"),
            ("R", "fix \"host key changed\" (ssh-keygen -R)"),
            ("", "● up · ● down · ◆ answered by its jump · ○ checking"),
        ],
    ),
    (
        "keys",
        &[
            ("c", "create a key (ssh-keygen)"),
            ("y", "yank the public key"),
            ("Y", "install it on a host (ssh-copy-id)"),
            (
                "",
                "agent: loaded in ssh-agent · passphrase: asks to unlock",
            ),
        ],
    ),
    (
        "tunnels",
        &[
            ("↵", "on/off (ssh -N)"),
            ("c", "create a forward: -L, -R or -D"),
            ("e", "edit it"),
            ("d", "delete it, stopping it first"),
        ],
    ),
    (
        "mounts",
        &[
            ("↵", "mount, unmount (sshfs, fusermount -u)"),
            ("d", "delete it, unmounting it first"),
        ],
    ),
    (
        "settings",
        &[("↵", "change it"), ("d", "put it back to its default")],
    ),
    (
        "in a form",
        &[
            ("type", "fill the field, h/j/k/l included"),
            ("ctrl-j/k ↑↓", "the previous, next field"),
            ("tab shift-tab", "the next, previous field"),
            ("h/l ←→", "step a ‹ choice ›"),
            ("ctrl-o", "pick a key or a host for the field"),
            ("↵", "the next field, and submit on the last"),
            ("esc", "cancel"),
            ("", "paste user@host:port in Alias to fill the rest"),
        ],
    ),
    (
        "in a box",
        &[
            ("y n", "answer"),
            ("h/l ←→ tab", "move between the buttons"),
            ("↵", "select, or pick from a list"),
            ("j/k ↑↓", "move in a list, scroll an alert"),
            ("esc", "cancel or close"),
        ],
    ),
    (
        "in this help",
        &[
            ("j/k ↑↓", "scroll"),
            ("ctrl-d ctrl-u", "half a page down, up"),
            ("g G", "the top, the bottom"),
            ("esc q ?", "close"),
        ],
    ),
];

/// The width of the key column, so every description starts in one place.
const HELP_KEYS: usize = 16;

pub(super) fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (section, entries) in HELP {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            *section,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        for (keys, what) in *entries {
            if keys.is_empty() {
                lines.push(Line::styled(
                    format!("  {what}"),
                    Style::default().add_modifier(Modifier::DIM),
                ));
                continue;
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {keys:<HELP_KEYS$}"),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(*what),
            ]));
        }
    }
    lines
}

/// The help reader: the body scrolls under a key row that never moves, with a
/// scrollbar on the right border once it is taller than the box.
pub(super) fn render_help(f: &mut Frame, area: Rect, app: &mut App) {
    let lines = help_lines();
    let width = box_width(area.width);
    // The body, then a blank and the key row.
    let rect = box_area(area, width, box_height(lines.len() as u16 + 2, area.height));
    f.render_widget(Clear, rect);
    let block = box_block(Color::Cyan, "help");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let shown = inner.height.saturating_sub(2) as usize;
    // Clamped here, where the height is known, so scrolling past the end never
    // piles up presses that then take as many to undo.
    app.help_scroll = app.help_scroll.min(lines.len().saturating_sub(shown));
    let top = app.help_scroll;
    let body = Rect {
        height: shown as u16,
        ..inner
    };
    f.render_widget(Paragraph::new(lines[top..].to_vec()), body);
    let keys = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: 1,
        ..inner
    };
    f.render_widget(Paragraph::new(box_hint(READER_KEYS)), keys);
    if lines.len() > shown {
        vscrollbar(f, rect, lines.len(), top, shown);
    }
}

/// The spaces that fill a coloured cell out to `width`, left unstyled: the
/// selected row is drawn reversed, and coloured padding would turn into a
/// solid block of that colour.
fn pad(text: &str, width: usize) -> Span<'static> {
    Span::raw(" ".repeat(width.saturating_sub(text.chars().count())))
}
