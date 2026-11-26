//! First-run selection of local directories.
use super::preferences::ThemeColor;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    widgets::Paragraph,
};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct DirectorySetup {
    focus: usize,
    paths: [Option<PathBuf>; 2],
    pub status: String,
}

pub(super) enum Action<'a> {
    Stay,

    Finish(&'a Path, &'a Path),
}

fn layout(area: Rect) -> [Rect; 5] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(super::page("", ThemeColor::default()).inner(area))
}

impl DirectorySetup {
    pub fn render(&self, frame: &mut Frame<'_>, color: ThemeColor) {
        let area = frame.area();
        frame.render_widget(super::page("首次设置", color), area);
        let [state, work, status, finish, footer] = layout(area);
        for (index, (label, rect)) in [
            ("本机状态目录", state),
            ("队伍工作目录", work),
            ("1 完成配置", finish),
        ]
        .into_iter()
        .enumerate()
        {
            let text = match self.paths.get(index) {
                Some(path) => format!(
                    "{label}  {}",
                    path.as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "未选择".into())
                ),
                None => label.into(),
            };
            let line = Paragraph::new(text);
            let line = if index == 2 { line.centered() } else { line };
            frame.render_widget(
                if index == self.focus {
                    line.style(color.highlight())
                } else {
                    line
                },
                rect,
            );
        }
        frame.render_widget(
            Paragraph::new(if self.focus == 2 {
                "enter · 完成配置"
            } else {
                "enter · 选择文件夹"
            }),
            footer,
        );
        let hint = if self.status.is_empty() {
            match self.focus {
                0 => "选择一个文件夹，让 SYNOD 在这台电脑上保存草稿和同步记录. ",
                1 => "选择你平时编辑队伍文件的位置. 这个目录要与本机状态目录分开. ",
                _ => "确认这两个位置后，继续设置队伍. ",
            }
        } else {
            &self.status
        };
        frame.render_widget(
            Paragraph::new(hint).wrap(ratatui::widgets::Wrap { trim: false }),
            status,
        );
    }

    pub fn handle(
        &mut self,
        event: Event,
        area: Rect,
        pick: impl FnOnce() -> io::Result<Option<PathBuf>>,
    ) -> Action<'_> {
        match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                code if let Some(next) = super::focus_step(code, self.focus, 3, &{
                    let [a, b, _, c, _] = layout(area);
                    [(0, a), (1, b), (2, c)]
                }) =>
                {
                    self.focus = next;
                    return Action::Stay;
                }
                _ if super::shortcut(k, 1, false).is_some() => self.focus = 2,
                KeyCode::Enter => {}
                _ => return Action::Stay,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                let [state, work, _, finish, _] = layout(area);
                let Some(index) = [state, work, finish]
                    .iter()
                    .position(|rect| rect.contains(Position::new(m.column, m.row)))
                else {
                    return Action::Stay;
                };
                self.focus = index;
            }
            _ => return Action::Stay,
        }
        if let Some(slot) = self.paths.get_mut(self.focus) {
            match pick() {
                Ok(Some(path)) => {
                    *slot = Some(path);
                    self.status.clear()
                }
                Ok(None) => {}
                Err(e) => self.status = e.to_string(),
            }
            return Action::Stay;
        }
        match &self.paths {
            [Some(state), Some(work)] => Action::Finish(state, work),
            _ => {
                self.status = "请先选择本机状态目录和队伍工作目录".into();
                Action::Stay
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn mouse_hits_and_inactive_events_preserve_selection() {
        use crossterm::event::{KeyEvent, KeyModifiers, MouseEvent};
        let mut screen = DirectorySetup::default();
        let area = Rect::new(0, 0, 50, 10);
        let click = |row, kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: 2,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let left = MouseEventKind::Down(MouseButton::Left);
        screen.handle(click(3, left), area, || {
            Err(io::Error::other("picker unavailable"))
        });
        assert_eq!(screen.status, "picker unavailable");
        screen.handle(click(2, left), area, || Ok(Some("/state".into())));
        screen.handle(click(3, left), area, || Ok(Some("/work".into())));
        for event in [
            click(5, left),
            click(1, MouseEventKind::Moved),
            Event::Resize(40, 10),
            Event::Key(KeyCode::Char('x').into()),
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Enter,
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )),
        ] {
            assert!(matches!(
                screen.handle(event, area, || panic!("unexpected picker activation")),
                Action::Stay
            ));
        }
        assert!(matches!(
            screen.handle(click(8, left), area, || Ok(None)),
            Action::Finish(..)
        ));
        assert!(matches!(
            screen.handle(Event::Key(KeyCode::Esc.into()), area, || Ok(None)),
            Action::Stay
        ));
    }
    #[test]
    fn cancelled_reselection_and_centered_completion() -> io::Result<()> {
        let area = Rect::new(0, 0, 46, 10);
        let mut setup = DirectorySetup::default();
        let key = |code: KeyCode| Event::Key(code.into());
        setup.handle(key(KeyCode::BackTab), area, || Ok(None));
        assert!(matches!(
            setup.handle(key(KeyCode::Enter), area, || Ok(None)),
            Action::Stay
        ));
        setup.handle(key(KeyCode::Tab), area, || Ok(None));
        setup.handle(key(KeyCode::Enter), area, || Ok(Some("/first".into())));
        setup.handle(key(KeyCode::Enter), area, || Ok(None));
        setup.handle(key(KeyCode::Down), area, || Ok(None));
        setup.handle(key(KeyCode::Enter), area, || Ok(Some("/second".into())));
        setup.handle(key(KeyCode::Down), area, || Ok(None));
        assert!(
            matches!(setup.handle(key(KeyCode::Enter),area,||Ok(None)),Action::Finish(s,w) if s==Path::new("/first")&&w==Path::new("/second"))
        );
        let mut terminal = Terminal::new(TestBackend::new(46, 10)).unwrap();
        terminal
            .draw(|f| setup.render(f, ThemeColor::default()))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(20, 8)].symbol(), "完");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "首");
        assert!(
            buffer[(0, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(buffer[(0, 0)].fg, ThemeColor::default().rgb());
        assert!(
            (0..46).all(|x| buffer[(x, 1)].symbol() == "─"
                && buffer[(x, 1)].fg == ThemeColor::default().rgb())
        );
        assert_eq!(buffer[(45, 0)].symbol(), " ");
        assert_eq!(buffer[(0, 8)].bg, buffer[(45, 8)].bg);
        assert_eq!(buffer[(0, 8)].bg, ratatui::style::Color::Rgb(154, 176, 194));
        Ok(())
    }
}
