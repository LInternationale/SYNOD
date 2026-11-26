//! Editable text with a visible insertion point at the end of the current input.
use super::preferences::ThemeColor;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub(super) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    theme: ThemeColor,
) {
    if area.is_empty() {
        return;
    }
    let [r, g, b] = theme.0;
    let bright = 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) >= 128000;
    let ink = if bright { Color::Black } else { Color::White };
    let style = if focused {
        theme.highlight().fg(ink)
    } else {
        Style::default()
    };
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut width = 0;
    for (index, line) in format!("{label}  {value}").split('\n').enumerate() {
        if index > 0 {
            rows.push(Line::raw(std::mem::take(&mut row)));
            width = 0;
        }
        let text = Span::raw(line);
        for glyph in text.styled_graphemes(Style::default()) {
            let cols = Span::raw(glyph.symbol).width() as u16;
            if width + cols > area.width {
                rows.push(Line::raw(std::mem::take(&mut row)));
                width = 0;
            }
            row.push_str(glyph.symbol);
            width = width.saturating_add(cols);
        }
    }
    if focused && width >= area.width {
        rows.push(Line::raw(std::mem::take(&mut row)));
        width = 0;
    }
    let cursor_color = if bright {
        Color::Rgb(0, 32, 100)
    } else {
        Color::Rgb(255, 224, 128)
    };
    let caret = Span::styled("▏", Style::default().fg(cursor_color));
    let mut last = Line::raw(row);
    if focused {
        last.spans.push(caret)
    }
    rows.push(last);
    let start = if focused {
        rows.len().saturating_sub(usize::from(area.height))
    } else {
        0
    };
    let cursor_y = (rows.len() - start - 1).min(usize::from(area.height - 1)) as u16;
    frame.render_widget(Paragraph::new(rows.split_off(start)).style(style), area);
    if focused {
        frame.set_cursor_position((area.x + width, area.y + cursor_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn visible_caret_tracks_wrapped_wide_input_and_leaves_unfocused_fields() {
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).unwrap();
        let theme = ThemeColor([230, 240, 250]);
        terminal
            .draw(|f| render(f, f.area(), "名", "甲乙丙\n很好", true, theme))
            .unwrap();
        let cells = terminal.backend().buffer();
        assert_eq!(cells[(4, 1)].symbol(), "▏");
        assert_ne!(cells[(0, 1)].fg, cells[(4, 1)].fg);
        assert_eq!(cells[(0, 1)].symbol(), "很");
        terminal
            .draw(|f| render(f, f.area(), "名", "a", false, theme))
            .unwrap();
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|c| c.symbol() != "▏")
        );
    }
}
