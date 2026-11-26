//! Combined draft and publication list with board actions.

use super::{BoardScreen, LetterView};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::Modifier,
    widgets::Paragraph,
};

/// A draft copy from the session's active identity.
pub struct DraftView<'a> {
    pub reference: &'a str,
    pub title: &'a str,
}

/// A transition requested by the bulletin board.
#[derive(Debug, PartialEq, Eq)]
pub enum BoardAction<'a> {
    Compose,
    Preferences,
    Workspace,
    OpenDraft(&'a str),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    List,
    Compose,
    Preferences,
    Workspace,
}

impl Focus {
    fn next(self) -> Self {
        match self {
            Self::List => Self::Compose,
            Self::Compose => Self::Preferences,
            Self::Preferences => Self::Workspace,
            Self::Workspace => Self::List,
        }
    }

    fn action(self) -> Option<BoardAction<'static>> {
        match self {
            Self::List => None,
            Self::Compose => Some(BoardAction::Compose),
            Self::Preferences => Some(BoardAction::Preferences),
            Self::Workspace => Some(BoardAction::Workspace),
        }
    }
}

const BUTTONS: [(Focus, &str); 3] = [
    (Focus::Compose, "写信"),
    (Focus::Preferences, "偏好"),
    (Focus::Workspace, "工作区"),
];

fn layout(area: Rect) -> (Rect, [Rect; 3]) {
    let [list, bar] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    (
        list,
        Layout::horizontal([Constraint::Ratio(1, 3); 3]).areas(bar),
    )
}

/// Session snapshot projected into display rows. Keys are isolated by source.
pub struct BoardRows<'a> {
    drafts: &'a [DraftView<'a>],
    rows: Vec<(String, String, &'a str)>,
}

impl<'a> BoardRows<'a> {
    pub fn new(drafts: &'a [DraftView<'a>], published: &[LetterView<'a>]) -> Self {
        let rows = drafts
            .iter()
            .map(|d| {
                (
                    format!("draft:{}", d.reference),
                    format!("【草稿】{}", d.title),
                    "",
                )
            })
            .chain(
                published
                    .iter()
                    .map(|l| (format!("letter:{}", l.id), l.title.into(), l.body)),
            )
            .collect();
        Self { drafts, rows }
    }

    fn views(&self) -> Vec<LetterView<'_>> {
        self.rows
            .iter()
            .map(|(id, title, body)| LetterView { id, title, body })
            .collect()
    }
}

/// Bulletin board with keyboard focus and mouse hit testing.
#[derive(Default)]
pub struct Bulletin {
    screen: BoardScreen,
    focus: Focus,
}

impl Bulletin {
    pub fn render(&mut self, frame: &mut Frame<'_>, rows: &BoardRows<'_>) {
        let views = rows.views();
        self.screen.refresh(&views);
        if matches!(self.screen, BoardScreen::Detail(_)) {
            self.screen.render(frame, &views);
            return;
        }
        let (list, buttons) = layout(frame.area());
        self.screen.render_area(frame, list, &views);
        for ((focus, label), area) in BUTTONS.into_iter().zip(buttons) {
            let widget = Paragraph::new(label).centered();
            let widget = if focus == self.focus {
                widget.style(Modifier::REVERSED)
            } else {
                widget
            };
            frame.render_widget(widget, area);
        }
    }

    pub fn handle<'a>(
        &mut self,
        event: Event,
        area: Rect,
        rows: &BoardRows<'a>,
    ) -> Option<BoardAction<'a>> {
        let views = rows.views();
        self.screen.refresh(&views);
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if matches!(self.screen, BoardScreen::Detail(_)) {
                    self.screen.navigate(key.code, &views);
                } else if key.code == KeyCode::Tab {
                    self.focus = self.focus.next();
                } else if key.code == KeyCode::Enter && self.focus != Focus::List {
                    return self.focus.action();
                } else if self.focus == Focus::List {
                    if key.code == KeyCode::Enter {
                        if let Some(draft) = self
                            .screen
                            .position(&views)
                            .and_then(|i| rows.drafts.get(i))
                        {
                            return Some(BoardAction::OpenDraft(draft.reference));
                        }
                    }
                    self.screen.navigate(key.code, &views);
                }
            }
            Event::Mouse(mouse)
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && !matches!(self.screen, BoardScreen::Detail(_)) =>
            {
                let point = Position::new(mouse.column, mouse.row);
                let (list, buttons) = layout(area);
                if list.contains(point) {
                    self.focus = Focus::List;
                }
                for ((focus, _), button) in BUTTONS.into_iter().zip(buttons) {
                    if button.contains(point) {
                        self.focus = focus;
                        return focus.action();
                    }
                }
            }
            _ => {}
        }
        None
    }
}
