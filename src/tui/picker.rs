//! Picking a value the app already knows (a host, a key) instead of retyping it.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::*;

/// A modal list picker: choose an existing value instead of typing it. Anything
/// the app already knows (config hosts, keys) must be picked, never re-typed.
pub(crate) struct Picker {
    pub(crate) title: String,
    pub(crate) items: Vec<String>,
    pub(crate) idx: usize,
    pub(crate) action: PickerAction,
}

pub(crate) enum PickerAction {
    CopyKeyTo {
        key: PathBuf,
    },
    NewKeyType,
    /// Write the chosen value into the active wizard's field at this index (used
    /// to pick a key for IdentityFile instead of typing the path).
    FillField {
        field: usize,
    },
}

pub(super) fn render_picker(f: &mut Frame, area: Rect, p: &Picker) {
    // Ten rows of list at most, so a long host list scrolls inside the box
    // rather than growing one that swallows the screen. Two more for the blank
    // and the keys, which live in the body like every other kind of box.
    let rows = (p.items.len() as u16).clamp(1, 10);
    let rect = box_area(
        area,
        box_width(area.width),
        box_height(rows + 2, area.height),
    );
    f.render_widget(Clear, rect);

    // The frame is drawn on its own so the list and the key line can share the
    // body: a List cannot hold a trailing line of its own.
    let block = super::widgets::box_block(Color::Cyan, &p.title);
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let (list_area, hint_area) = (
        Rect {
            height: inner.height.saturating_sub(2),
            ..inner
        },
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        },
    );

    let items: Vec<ListItem> = p
        .items
        .iter()
        .map(|it| ListItem::new(Span::raw(it.clone())))
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    // A local state gives us scrolling for free when there are many hosts.
    let mut state = ListState::default();
    state.select(Some(p.idx));
    f.render_stateful_widget(list, list_area, &mut state);
    f.render_widget(
        Paragraph::new(super::widgets::box_hint(
            "j/k ↑↓ move · enter pick · esc cancel",
        )),
        hint_area,
    );
}

impl App {
    /// Drive a modal list picker: the list keys move, Enter chooses, Esc bails.
    pub(super) fn picker_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.picker.as_ref().map(|p| p.items.len()).unwrap_or(0);
        if len == 0 {
            self.picker = None;
            return None;
        }
        match key.code {
            KeyCode::Esc => {
                self.picker = None;
                self.set_status("cancelled");
            }
            KeyCode::Char('c') if ctrl => {
                self.picker = None;
                self.set_status("cancelled");
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let p = self.picker.as_mut().unwrap();
                p.idx = (p.idx + 1) % len;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let p = self.picker.as_mut().unwrap();
                p.idx = (p.idx + len - 1) % len;
            }
            KeyCode::Enter => return self.submit_picker(),
            _ => {}
        }
        None
    }

    pub(super) fn submit_picker(&mut self) -> Option<PendingRun> {
        let picker = self.picker.take()?;
        let choice = picker.items.get(picker.idx)?.clone();
        match picker.action {
            // Choosing a type just opens the name/comment wizard for it.
            PickerAction::NewKeyType => {
                let kind = if picker.idx == 0 { "ed25519" } else { "rsa" };
                self.prompt = Some(Prompt::new_key(kind));
                None
            }
            PickerAction::CopyKeyTo { key } => Some(PendingRun {
                argv: vec![
                    "ssh-copy-id".into(),
                    "-i".into(),
                    key.to_string_lossy().into_owned(),
                    choice.clone(),
                ],
                label: format!("ssh-copy-id -> {choice}"),
                connect: None,
            }),
            PickerAction::FillField { field } => {
                if let Some(f) = self.prompt.as_mut().and_then(|p| p.fields.get_mut(field)) {
                    f.value = choice;
                }
                None
            }
        }
    }
}
