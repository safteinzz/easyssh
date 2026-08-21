//! Pane furniture with no idea what it is drawing: titled blocks, centering and
//! the wrapped-row count a box needs to size itself.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Rough count of how many rows `text` needs when word-wrapped to `width`,
/// matching how ratatui's `Wrap` breaks on spaces. Used to size modal boxes.
pub(super) fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut rows = 1usize;
    let mut col = 0usize;
    // Split on single spaces rather than runs of whitespace: the indent and the
    // gap after "runs" are real columns, and collapsing them undercounted the
    // rows enough to clip the line off the bottom of a wizard.
    for (i, word) in text.split(' ').enumerate() {
        let w = word.chars().count();
        // Only the very first token has no space in front of it. Keying this off
        // `col == 0` instead swallowed a column for every leading space, which
        // undercounted the rows and clipped the last line off a box.
        let need = if i == 0 { w } else { col + 1 + w };
        if need <= width {
            col = need;
        } else {
            rows += 1;
            col = w;
        }
        // A word longer than the whole line wraps across several rows.
        while col > width {
            rows += 1;
            col -= width;
        }
    }
    rows
}

/// A bordered block titled `Name (count)`.
pub(super) fn titled(name: &str, count: usize) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {name} ({count}) "))
}

/// Render a friendly empty-state message inside the body block.
pub(super) fn empty(f: &mut Frame, area: Rect, name: &str, msg: &str) {
    let para = Paragraph::new(msg)
        .block(titled(name, 0))
        .style(Style::default().add_modifier(Modifier::DIM))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

/// A rect centered in `area`, `pct_x`% wide and `height` rows tall.
pub(super) fn centered(area: Rect, pct_x: u16, height: u16) -> Rect {
    let w = area.width * pct_x / 100;
    let h = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

// ==========================================================================
// Terminal lifecycle + event loop
// ==========================================================================
