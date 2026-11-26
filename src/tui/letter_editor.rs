//! Compose viewport and insertion-point navigation.
use super::{
    live::{Action, Term, input},
    preferences::ThemeColor,
};
use crate::session::protocol::{Draft, LetterRow};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::io;

#[derive(Default)]
pub(super) struct LetterEditor {
    focus: usize,
    editing: bool,
    caret: usize,
    offset: usize,
}

struct VisualLine {
    start: usize,
    end: usize,
}

fn visual_lines(text: &str, width: usize) -> Vec<VisualLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut cols = 0;
    let mut offset = 0;
    for logical in text.split('\n') {
        for glyph in Span::raw(logical).styled_graphemes(Style::default()) {
            let w = Span::raw(glyph.symbol).width();
            if cols > 0 && cols + w > width {
                lines.push(VisualLine { start, end: offset });
                start = offset;
                cols = 0;
            }
            cols += w;
            offset += glyph.symbol.len();
        }
        lines.push(VisualLine { start, end: offset });
        if cols >= width {
            lines.push(VisualLine {
                start: offset,
                end: offset,
            });
        }
        offset += 1;
        start = offset;
        cols = 0;
    }
    lines
}
fn column_offset(text: &str, column: usize) -> usize {
    let mut cols = 0;
    let mut bytes = 0;
    for glyph in Span::raw(text).styled_graphemes(Style::default()) {
        cols += Span::raw(glyph.symbol).width();
        if cols > column {
            break;
        }
        bytes += glyph.symbol.len();
    }
    bytes
}
pub(super) fn names(
    ids: impl Iterator<Item = impl AsRef<str>>,
    choices: &[(&str, &str)],
) -> String {
    ids.filter(|id| !id.as_ref().is_empty())
        .map(|id| {
            choices
                .iter()
                .find(|(key, _)| *key == id.as_ref())
                .map_or_else(|| id.as_ref().to_owned(), |(_, name)| name.to_string())
        })
        .collect::<Vec<_>>()
        .join("、")
}

impl LetterEditor {
    pub(super) fn interact(
        &mut self,
        t: &mut Term,
        color: ThemeColor,
        d: &mut Draft,
        roster: &[(&str, &str)],
        letters: &[LetterRow],
        message: &str,
    ) -> io::Result<Action> {
        let refs = letters
            .iter()
            .map(|l| (l.id.as_str(), l.title.as_str()))
            .collect::<Vec<_>>();
        loop {
            let save = 7 + d.attachments.len();
            let mut bounds = Vec::new();
            let mut page = 1;
            let mut end_body = 0;
            let mut max_offset = 0;
            let mut lines = Vec::new();
            let mut cursor_row = 0;
            t.draw(|f| {
                let header = super::page("写信", color);
                let area = header.inner(f.area());
                f.render_widget(header, f.area());
                let [subject, view, status, bar, hint] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(u16::from(!message.is_empty())),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .areas(area);
                let field = |label: &str, value: String, active| {
                    Line::from(vec![
                        Span::styled(
                            format!(
                                "{label}{}",
                                " ".repeat(8usize.saturating_sub(Span::raw(label).width()))
                            ),
                            if active {
                                color.highlight()
                            } else {
                                Style::default()
                            },
                        ),
                        Span::raw(value),
                    ])
                };
                f.render_widget(
                    Paragraph::new(field("标题", d.title.clone(), self.focus == 0)),
                    subject,
                );
                if self.focus == 0 && subject.width > 8 {
                    let width = usize::from(subject.width - 9);
                    let title = Span::raw(&d.title);
                    let glyphs = title.styled_graphemes(Style::default()).collect::<Vec<_>>();
                    let mut used = 0;
                    let start = glyphs
                        .iter()
                        .rposition(|g| {
                            used += Span::raw(g.symbol).width();
                            used > width
                        })
                        .map_or(0, |i| i + 1);
                    let tail = glyphs[start..].iter().map(|g| g.symbol).collect::<String>();
                    let x = subject.x + 8 + Span::raw(&tail).width() as u16;
                    f.render_widget(Paragraph::new(field("标题", tail, true)), subject);
                    f.render_widget(
                        Span::styled("▏", Style::default().fg(color.rgb())),
                        Rect::new(x, subject.y, 1, 1),
                    );
                    f.set_cursor_position((x, subject.y));
                }
                bounds.push((0, subject));
                let viewport = view;
                let view = Rect {
                    width: view.width.saturating_sub(1),
                    ..view
                };
                lines = visual_lines(&d.body, usize::from(view.width.max(1)));
                cursor_row = lines
                    .partition_point(|line| line.start <= self.caret)
                    .saturating_sub(1);
                page = usize::from(view.height.max(1));
                let attachment_y = 5 + lines.len();
                end_body = attachment_y.saturating_sub(page);
                max_offset = (attachment_y + 1 + d.attachments.len()).saturating_sub(page);
                if self.editing {
                    let caret_y = 5 + cursor_row;
                    self.offset = self
                        .offset
                        .min(caret_y)
                        .max((caret_y + 1).saturating_sub(page));
                } else if (6..save).contains(&self.focus) {
                    let y = attachment_y + self.focus - 6;
                    self.offset = self.offset.min(y).max((y + 1).saturating_sub(page));
                }
                self.offset = self.offset.min(max_offset);
                super::scroll_gutter(f, viewport, self.offset, max_offset, color);
                let rect = |y: usize| {
                    (y >= self.offset && y < self.offset + usize::from(view.height)).then(|| {
                        Rect::new(view.x, view.y + (y - self.offset) as u16, view.width, 1)
                    })
                };
                for (i, (label, value)) in [
                    ("收件", names(std::iter::once(&d.to), roster)),
                    ("抄送", names(d.cc.split(','), roster)),
                    ("引用", names(d.references.iter(), &refs)),
                    ("子任务", names(d.dependencies.iter(), &refs)),
                    ("正文", String::new()),
                ]
                .into_iter()
                .enumerate()
                {
                    if let Some(r) = rect(i) {
                        f.render_widget(
                            Paragraph::new(field(label, value, self.focus == i + 1)),
                            r,
                        );
                        bounds.push((i + 1, r));
                    }
                }
                for (i, line) in lines
                    .iter()
                    .enumerate()
                    .skip(self.offset.saturating_sub(5))
                    .take(page)
                {
                    if let Some(r) = rect(i + 5) {
                        f.render_widget(Paragraph::new(&d.body[line.start..line.end]), r);
                        bounds.push((5, r));
                        if self.editing && i == cursor_row && r.width > 0 {
                            let col = Span::raw(&d.body[line.start..self.caret]).width();
                            let x = r.x + (col as u16).min(r.width - 1);
                            f.buffer_mut()[(x, r.y)].set_style(color.highlight());
                            f.set_cursor_position((x, r.y));
                        }
                    }
                }
                if let Some(r) = rect(attachment_y) {
                    f.render_widget(
                        Paragraph::new(field("附件", String::new(), self.focus == 6)),
                        r,
                    );
                    bounds.push((6, r));
                }
                for (i, a) in d.attachments.iter().enumerate() {
                    if let Some(r) = rect(attachment_y + 1 + i) {
                        f.render_widget(
                            Paragraph::new(a.filename.as_str()).style(if self.focus == 7 + i {
                                color.highlight()
                            } else {
                                Style::default()
                            }),
                            r,
                        );
                        bounds.push((7 + i, r));
                    }
                }
                for (i, r) in super::action_cells(bar, 2).into_iter().enumerate() {
                    f.render_widget(
                        Paragraph::new(["F1 存草稿", "F2 寄信"][i])
                            .centered()
                            .style(if self.focus == save + i {
                                color.highlight()
                            } else {
                                Style::default()
                            }),
                        r,
                    );
                    bounds.push((save + i, r));
                }
                let tip = match self.focus {
                    0 => "输入 · 编辑标题",
                    1..=4 => "enter · 选择",
                    5 if self.editing => "方向键 · 移动光标    esc · 结束编辑",
                    5 => "↑↓ · 滚动正文    enter · 编辑正文",
                    6 => "a · 添加附件",
                    n if n < save => "x · 删除附件",
                    _ => "enter · 执行",
                };
                f.render_widget(Paragraph::new(message), status);
                f.render_widget(Paragraph::new(tip), hint);
            })?;
            let old = self.focus;
            let mut activate = false;
            match input()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if let Some(i) = super::shortcut(k, 2, false) {
                        return Ok(Action::Button(5 + i));
                    }
                    if self.editing {
                        let line = &lines[cursor_row];
                        let col = Span::raw(&d.body[line.start..self.caret]).width();
                        match k.code {
                            KeyCode::Esc => self.editing = false,
                            KeyCode::Char(ch) => {
                                d.body.insert(self.caret, ch);
                                self.caret += ch.len_utf8();
                            }
                            KeyCode::Enter => {
                                d.body.insert(self.caret, '\n');
                                self.caret += 1;
                            }
                            KeyCode::Backspace | KeyCode::Left => {
                                if let Some((p, _)) =
                                    d.body[..self.caret].char_indices().next_back()
                                {
                                    if k.code == KeyCode::Backspace {
                                        d.body.replace_range(p..self.caret, "");
                                    }
                                    self.caret = p;
                                }
                            }
                            KeyCode::Right => {
                                if let Some(c) = d.body[self.caret..].chars().next() {
                                    self.caret += c.len_utf8();
                                }
                            }
                            KeyCode::Delete => {
                                if self.caret < d.body.len() {
                                    d.body.remove(self.caret);
                                }
                            }
                            KeyCode::Home => self.caret = line.start,
                            KeyCode::End => self.caret = line.end,
                            KeyCode::Up | KeyCode::Down => {
                                let row = if k.code == KeyCode::Up {
                                    cursor_row.saturating_sub(1)
                                } else {
                                    (cursor_row + 1).min(lines.len() - 1)
                                };
                                let l = &lines[row];
                                self.caret = l.start + column_offset(&d.body[l.start..l.end], col);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match k.code {
                        KeyCode::Esc => return Ok(Action::Back),
                        KeyCode::Char(ch) if self.focus == 0 => d.title.push(ch),
                        KeyCode::Backspace if self.focus == 0 => {
                            d.title.pop();
                        }
                        KeyCode::Char('a') if self.focus == 6 => return Ok(Action::Button(4)),
                        KeyCode::Char('x') if (7..save).contains(&self.focus) => {
                            d.attachments.remove(self.focus - 7);
                            self.focus = self.focus.min(save - 2);
                        }
                        KeyCode::Enter => activate = true,
                        KeyCode::PageUp => {
                            self.focus = 5;
                            self.offset = self.offset.saturating_sub(page);
                        }
                        KeyCode::PageDown => {
                            self.focus = 5;
                            self.offset = (self.offset + page).min(end_body);
                        }
                        KeyCode::Up if self.focus == 5 && self.offset > 0 => self.offset -= 1,
                        KeyCode::Down | KeyCode::Tab
                            if self.focus == 5 && self.offset < end_body =>
                        {
                            self.offset += 1
                        }
                        KeyCode::Down | KeyCode::Tab if self.focus < save => self.focus += 1,
                        KeyCode::Up | KeyCode::BackTab if self.focus > 0 && self.focus <= save => {
                            self.focus -= 1
                        }
                        code if let Some(next) =
                            super::focus_step(code, self.focus, save + 2, &bounds) =>
                        {
                            self.focus = next
                        }
                        _ => {}
                    }
                    if matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
                        continue;
                    }
                }
                Event::Mouse(m)
                    if matches!(
                        m.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
                {
                    self.editing = false;
                    self.focus = 5;
                    self.offset = if m.kind == MouseEventKind::ScrollUp {
                        self.offset.saturating_sub(3)
                    } else {
                        (self.offset + 3).min(max_offset)
                    };
                    continue;
                }
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((id, _)) = bounds
                        .iter()
                        .find(|(_, r)| r.contains(Position::new(m.column, m.row)))
                    {
                        self.focus = *id;
                        activate = true;
                        self.editing = false;
                    }
                }
                _ => {}
            }
            if self.focus <= 4 {
                self.offset = 0;
            }
            if self.focus == 5 && (old != 5 || activate) {
                self.editing = true;
            }
            if activate {
                match self.focus {
                    1..=4 => return Ok(Action::Button(self.focus - 1)),
                    n if n >= save => return Ok(Action::Button(5 + n - save)),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapped_caret_respects_wide_characters_and_empty_lines() {
        let text = "ab中\n\n末";
        let rows = visual_lines(text, 3);
        assert_eq!(
            rows.iter()
                .map(|r| &text[r.start..r.end])
                .collect::<Vec<_>>(),
            ["ab", "中", "", "末"]
        );
        assert_eq!(column_offset("中a", 1), 0);
        assert_eq!(column_offset("中a", 2), 3);
    }
}
