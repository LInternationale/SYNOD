//! Layout and keyboard state of the letter editor.
use super::{
    checkbox,
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
    scroll: usize,
    follow: bool,
}
fn wrapped(text: &str, width: u16) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut x = 0;
    for line in text.split('\n').enumerate() {
        if line.0 > 0 {
            rows.push(String::new());
            x = 0;
        }
        for glyph in Span::raw(line.1).styled_graphemes(Style::default()) {
            let w = Span::raw(glyph.symbol).width();
            if x > 0 && x + w > usize::from(width) {
                rows.push(String::new());
                x = 0
            }
            if let Some(row) = rows.last_mut() {
                row.push_str(glyph.symbol)
            }
            x += w;
        }
    }
    rows
}
fn names(ids: impl Iterator<Item = impl AsRef<str>>, choices: &[(&str, &str)]) -> String {
    ids.filter_map(|id| {
        choices
            .iter()
            .find(|(key, _)| *key == id.as_ref())
            .map(|(_, label)| *label)
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
        let save = 6 + letters.len();
        let count = save + 2;
        loop {
            let mut bounds = Vec::new();
            let mut body_page = 1;
            let mut max_scroll = 0;
            t.draw(|f| {
                let frame = super::page("写信", color);
                let area = frame.inner(f.area());
                f.render_widget(frame, f.area());
                let [main, feedback, buttons, hint] = Layout::vertical([
                    Constraint::Min(0),
                    Constraint::Length(u16::from(!message.is_empty())),
                    Constraint::Length(super::action_height(area.width, 2)),
                    Constraint::Length(1),
                ])
                .areas(area);
                let mut rows = wrapped(&d.body, main.width.max(1));
                if self.focus == 4
                    && rows.last().is_some_and(|line| {
                        Span::raw(line).width() >= usize::from(main.width.max(1))
                    })
                {
                    rows.push(String::new());
                }
                let task_height = if letters.is_empty() {
                    1
                } else {
                    1 + letters.len().min(4) as u16
                };
                let mut collapsed = self.scroll > 0;
                if self.focus == 4 && self.follow {
                    let available = main
                        .height
                        .saturating_sub(1 + 3 + 1 + task_height + 1)
                        .max(1);
                    collapsed = rows.len() > usize::from(available);
                }
                let [subject, headers, body_label, body, attachments, tasks] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(if collapsed { 0 } else { 3 }),
                    Constraint::Length(u16::from(!collapsed)),
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(task_height),
                ])
                .areas(main);
                let field = |f: &mut ratatui::Frame<'_>,
                             rect: Rect,
                             label: &str,
                             value: &str,
                             active: bool| {
                    let line = Line::from(vec![
                        Span::styled(
                            format!("{label}    "),
                            if active {
                                color.highlight()
                            } else {
                                Style::default()
                            },
                        ),
                        Span::raw(value.to_string()),
                    ]);
                    f.render_widget(Paragraph::new(line), rect);
                };
                field(f, subject, "标题", &d.title, self.focus == 0);
                if self.focus == 0 && subject.width > 0 {
                    let start = Span::raw("标题    ").width() as u16;
                    let mut cols = 0usize;
                    let available = subject.width.saturating_sub(start + 1) as usize;
                    let mut tail = Vec::new();
                    let title = Span::raw(&d.title);
                    let glyphs = title.styled_graphemes(Style::default()).collect::<Vec<_>>();
                    for glyph in glyphs.iter().rev() {
                        let w = Span::raw(glyph.symbol).width();
                        if cols + w > available {
                            break;
                        }
                        cols += w;
                        tail.push(glyph.symbol);
                    }
                    tail.reverse();
                    field(f, subject, "标题", &tail.concat(), true);
                    if start < subject.width {
                        let x = subject.x + start + cols as u16;
                        f.render_widget(
                            Span::styled("▏", Style::default().fg(color.rgb())),
                            Rect::new(x, subject.y, 1, 1),
                        );
                        f.set_cursor_position((x, subject.y));
                    }
                }
                bounds.push((0, subject));
                for (i, (label, value)) in [
                    ("收件", names(std::iter::once(&d.to), roster)),
                    ("抄送", names(d.cc.split(','), roster)),
                    ("引用", names(d.references.iter(), &refs)),
                ]
                .into_iter()
                .enumerate()
                {
                    if i < usize::from(headers.height) {
                        let rect = Rect::new(headers.x, headers.y + i as u16, headers.width, 1);
                        field(f, rect, label, &value, self.focus == i + 1);
                        bounds.push((i + 1, rect));
                    }
                }
                field(f, body_label, "正文", "", self.focus == 4);
                body_page = usize::from(body.height.max(1));
                max_scroll = rows
                    .len()
                    .saturating_sub(body_page)
                    .max(usize::from(collapsed && rows.len() > 1));
                if self.focus == 4 && self.follow {
                    self.scroll = max_scroll;
                } else {
                    self.scroll = self.scroll.min(max_scroll);
                }
                f.render_widget(
                    Paragraph::new(
                        rows.iter()
                            .skip(self.scroll)
                            .take(body_page)
                            .cloned()
                            .map(Line::raw)
                            .collect::<Vec<_>>(),
                    ),
                    body,
                );
                if self.focus == 4 && self.scroll + body_page >= rows.len() && !body.is_empty() {
                    let width = rows.last().map_or(0, |r| Span::raw(r).width());
                    let x = body.x + (width as u16).min(body.width - 1);
                    let y = body.y
                        + ((rows.len() - 1).saturating_sub(self.scroll) as u16)
                            .min(body.height - 1);
                    f.render_widget(
                        Span::styled("▏", Style::default().fg(color.rgb())),
                        Rect::new(x, y, 1, 1),
                    );
                    f.set_cursor_position((x, y));
                }
                bounds.push((4, body));
                let [add, list] = Layout::horizontal([Constraint::Length(10), Constraint::Min(0)])
                    .areas(attachments);
                f.render_widget(
                    Paragraph::new("1 添加附件")
                        .centered()
                        .style(if self.focus == 5 {
                            color.highlight()
                        } else {
                            Style::default()
                        }),
                    add,
                );
                f.render_widget(
                    Paragraph::new(
                        d.attachments
                            .iter()
                            .map(|a| a.filename.as_str())
                            .collect::<Vec<_>>()
                            .join("、"),
                    ),
                    list,
                );
                bounds.push((5, add));
                f.render_widget(Paragraph::new("子任务"), tasks);
                let visible = usize::from(tasks.height.saturating_sub(1));
                let first = if (6..save).contains(&self.focus) {
                    (self.focus - 5).saturating_sub(visible)
                } else {
                    0
                };
                for (i, l) in letters.iter().enumerate().skip(first).take(visible) {
                    let rect = Rect::new(tasks.x, tasks.y + 1 + (i - first) as u16, tasks.width, 1);
                    f.render_widget(
                        Paragraph::new(checkbox(
                            d.dependencies.contains(&l.id),
                            &l.title,
                            self.focus == 6 + i,
                            color,
                        )),
                        rect,
                    );
                    bounds.push((6 + i, rect));
                }
                for (i, label) in ["存草稿", "寄信"].iter().enumerate() {
                    let rect = super::action_cells(buttons, 2)[i];
                    f.render_widget(
                        Paragraph::new(format!("{} {label}", i + 2))
                            .centered()
                            .style(if self.focus == save + i {
                                color.highlight()
                            } else {
                                Style::default()
                            }),
                        rect,
                    );
                    bounds.push((save + i, rect));
                }
                let tip = match self.focus {
                    0 => "输入 · 编辑标题",
                    1..=3 => "enter · 选择",
                    4 => "PgUp/PgDn · 滚动正文",
                    5 => "enter · 添加附件",
                    n if n < save => "space · 切换子任务",
                    _ => "enter · 执行",
                };
                f.render_widget(Paragraph::new(message), feedback);
                f.render_widget(Paragraph::new(tip), hint);
            })?;
            let execute = match input()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    _ if let Some(i) = super::shortcut(k, 3, matches!(self.focus, 0 | 4)) => {
                        return Ok(Action::Button(4 + i));
                    }
                    KeyCode::Esc => return Ok(Action::Back),
                    KeyCode::PageUp | KeyCode::PageDown => {
                        self.follow = false;
                        self.scroll = if k.code == KeyCode::PageUp {
                            self.scroll.saturating_sub(body_page)
                        } else {
                            (self.scroll + body_page).min(max_scroll)
                        };
                        false
                    }
                    KeyCode::Enter if self.focus == 4 => {
                        d.body.push('\n');
                        self.follow = true;
                        false
                    }
                    KeyCode::Char(ch) if matches!(self.focus, 0 | 4) => {
                        if self.focus == 0 {
                            d.title.push(ch)
                        } else {
                            d.body.push(ch);
                            self.follow = true
                        }
                        false
                    }
                    KeyCode::Backspace if matches!(self.focus, 0 | 4) => {
                        if self.focus == 0 {
                            d.title.pop();
                        } else {
                            d.body.pop();
                            self.follow = true
                        }
                        false
                    }
                    KeyCode::Enter => true,
                    KeyCode::Char(' ') if (6..save).contains(&self.focus) => true,
                    KeyCode::Up if self.focus > 6 && self.focus < save => {
                        self.focus -= 1;
                        false
                    }
                    KeyCode::Down if self.focus >= 6 && self.focus + 1 < save => {
                        self.focus += 1;
                        false
                    }
                    code if let Some(next) =
                        super::focus_step(code, self.focus, count, &bounds) =>
                    {
                        self.focus = next;
                        if next < 4 {
                            self.scroll = 0;
                            self.follow = false
                        }
                        false
                    }
                    _ => false,
                },
                Event::Mouse(m)
                    if matches!(
                        m.kind,
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                    ) =>
                {
                    self.follow = false;
                    self.scroll = if m.kind == MouseEventKind::ScrollUp {
                        self.scroll.saturating_sub(3)
                    } else {
                        (self.scroll + 3).min(max_scroll)
                    };
                    false
                }
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((i, _)) = bounds
                        .iter()
                        .find(|(_, r)| r.contains(Position::new(m.column, m.row)))
                    {
                        self.focus = *i;
                        !matches!(*i, 0 | 4)
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if execute {
                match self.focus {
                    1..=3 => return Ok(Action::Button(self.focus - 1)),
                    5 => return Ok(Action::Button(4)),
                    n if (6..save).contains(&n) => {
                        let id = &letters[n - 6].id;
                        if d.dependencies.contains(id) {
                            d.dependencies.retain(|v| v != id)
                        } else {
                            d.dependencies.push(id.clone())
                        }
                    }
                    n if n >= save => return Ok(Action::Button(5 + n - save)),
                    _ => {}
                }
            }
        }
    }
}
