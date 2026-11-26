//! Preference tabs and in-page editing.
use super::{
    input_field,
    live::{Term, input, workspace},
    preferences::{ThemeColor, ThemeEditor},
};
use crate::session::{protocol::*, transport::Client};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    widgets::Paragraph,
};
use std::{io, path::Path};

const NAMES: [&str; 4] = ["成员", "远端", "工作区", "风格"];
pub(super) fn tabs(
    f: &mut Frame<'_>,
    area: Rect,
    current: usize,
    focused: bool,
    color: ThemeColor,
) -> Vec<Rect> {
    let cells = super::action_cells(area, 4);
    for (i, rect) in cells.iter().enumerate() {
        let style = if i != current {
            Style::default()
        } else if focused {
            color.highlight()
        } else {
            Style::default()
                .fg(color.rgb())
                .add_modifier(Modifier::UNDERLINED)
        };
        f.render_widget(Paragraph::new(NAMES[i]).centered().style(style), *rect);
    }
    cells
}

pub(super) fn run(
    t: &mut Term,
    client: &mut Client,
    color: &mut ThemeColor,
    root: &Path,
) -> io::Result<()> {
    let Reply::Team(Some(p)) = client.request(Request::Profile)? else {
        return Ok(());
    };
    let Reply::Interval(period) = client.request(Request::Interval)? else {
        return Err(io::Error::other("Missing sync interval"));
    };
    let mut member = [p.human, p.assistant, String::new(), String::new()];
    let mut interval = [period.to_string()];
    let mut theme = ThemeEditor::new(*color);
    let mut tab = 0usize;
    let mut focus = 0usize;
    let mut message = String::new();
    let mut replace = true;
    let mut roster = p
        .team
        .roster()
        .iter()
        .map(|(id, name)| format!("{name} ({id})"))
        .collect::<Vec<_>>()
        .join(" · ");
    loop {
        if tab == 2 {
            match workspace(t, client, *color, root, true)? {
                Some(next) => {
                    tab = next;
                    focus = 0;
                    message.clear();
                    continue;
                }
                None => return Ok(()),
            }
        }
        let (labels, actions): (&[&str], &[&str]) = match tab {
            0 => (
                &["本机人类", "本机 AI", "新人类显示名", "新 AI 显示名"],
                &["应用身份", "添加成员"],
            ),
            1 => (&["同步间隔（秒）"], &["应用设置"]),
            _ => (&["R", "G", "B"], &["应用设置"]),
        };
        let values: &mut [String] = match tab {
            0 => &mut member,
            1 => &mut interval,
            _ => &mut theme.channels,
        };
        let count = 1 + values.len() + actions.len();
        let mut bounds = Vec::new();
        let mut tab_hits = Vec::new();
        t.draw(|f| {
            let page = super::page("偏好", *color);
            let inner = page.inner(f.area());
            f.render_widget(page, f.area());
            let [strip, info, fields, bar, status, hint] = Layout::vertical([
                Constraint::Length(super::action_height(inner.width, 4)),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(super::action_height(inner.width, actions.len())),
                Constraint::Length(u16::from(!message.is_empty())),
                Constraint::Length(1),
            ])
            .areas(inner);
            tab_hits = tabs(f, strip, tab, focus == 0, *color);
            bounds.push((0, strip));
            f.render_widget(
                Paragraph::new(match tab {
                    0 => roster.as_str(),
                    1 => p.remote.as_str(),
                    _ => "主题色",
                }),
                info,
            );
            let first = focus
                .saturating_sub(usize::from(fields.height))
                .min(values.len().saturating_sub(1));
            for i in (first..values.len()).take(fields.height as usize) {
                let rect = Rect::new(fields.x, fields.y + (i - first) as u16, fields.width, 1);
                input_field::render(f, rect, labels[i], &values[i], focus == i + 1, *color);
                bounds.push((i + 1, rect));
            }
            for (i, rect) in super::action_cells(bar, actions.len()).iter().enumerate() {
                let id = 1 + values.len() + i;
                f.render_widget(
                    Paragraph::new(format!("{} {}", i + 1, actions[i]))
                        .centered()
                        .style(if focus == id {
                            color.highlight()
                        } else {
                            Style::default()
                        }),
                    *rect,
                );
                bounds.push((id, *rect));
            }
            f.render_widget(Paragraph::new(message.as_str()), status);
            f.render_widget(
                Paragraph::new(if focus == 0 {
                    "←/→ · 切换分类    ↓ · 配置"
                } else if focus <= values.len() {
                    "输入 · 修改    esc · 回到分类"
                } else {
                    "enter · 执行"
                }),
                hint,
            );
        })?;
        let apply = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc if focus == 0 => return Ok(()),
                KeyCode::Esc => {
                    focus = 0;
                    continue;
                }
                KeyCode::Left | KeyCode::Right if focus == 0 => {
                    tab = if k.code == KeyCode::Left {
                        tab.saturating_sub(1)
                    } else {
                        (tab + 1).min(3)
                    };
                    message.clear();
                    continue;
                }
                _ if let Some(i) =
                    super::shortcut(k, actions.len(), focus > 0 && focus <= values.len()) =>
                {
                    i
                }
                KeyCode::Enter if focus > values.len() => focus - values.len() - 1,
                KeyCode::Enter if focus == 0 => {
                    focus = 1;
                    replace = true;
                    continue;
                }
                code if let Some(next) = super::focus_step(code, focus, count, &bounds) => {
                    focus = next;
                    replace = true;
                    continue;
                }
                KeyCode::Char(ch) if focus > 0 && focus <= values.len() => {
                    let val = &mut values[focus - 1];
                    if replace {
                        val.clear();
                        replace = false
                    }
                    val.push(ch);
                    continue;
                }
                KeyCode::Backspace if focus > 0 && focus <= values.len() => {
                    values[focus - 1].pop();
                    replace = false;
                    continue;
                }
                _ => continue,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                let pos = Position::new(m.column, m.row);
                if let Some(i) = tab_hits.iter().position(|r| r.contains(pos)) {
                    tab = i;
                    focus = 0;
                    message.clear();
                    continue;
                }
                let Some((i, _)) = bounds.iter().find(|(_, r)| r.contains(pos)) else {
                    continue;
                };
                focus = *i;
                replace = true;
                if focus <= values.len() {
                    continue;
                }
                focus - values.len() - 1
            }
            _ => continue,
        };
        let result: io::Result<()> = match tab {
            0 if apply == 0 => client
                .request(Request::Identities {
                    human: member[0].clone(),
                    assistant: member[1].clone(),
                })
                .map(|_| ()),
            0 => client
                .request(Request::Register {
                    human: member[2].clone(),
                    assistant: member[3].clone(),
                    initial: false,
                })
                .map(|reply| {
                    if let Reply::Team(Some(p)) = reply {
                        roster = p
                            .team
                            .roster()
                            .iter()
                            .map(|(id, label)| format!("{label} ({id})"))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    }
                    member[2].clear();
                    member[3].clear();
                }),
            1 => interval[0]
                .parse()
                .map_err(|_| io::Error::other("请输入大于零的整数秒数"))
                .and_then(|seconds| client.request(Request::SetInterval(seconds)).map(|_| ())),
            _ => theme
                .apply(|v| client.request(Request::ApplyTheme(v.0)).map(|_| ()))
                .map(|_| *color = theme.active())
                .map_err(|e| io::Error::other(format!("{e:?}"))),
        };
        message = result.map_or_else(|e| e.to_string(), |_| "已应用".into());
    }
}
