//! Combined draft and publication list with board actions.

use super::{BoardScreen, LetterView};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
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
    QuickReply(String),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    List,
    Compose,
    Preferences,
    Workspace,
    Reply,
}

impl Focus {
    fn move_focus(self, code: KeyCode, reply: bool) -> Option<Self> {
        let order = [
            Self::List,
            Self::Compose,
            Self::Preferences,
            Self::Workspace,
            Self::Reply,
        ];
        let index = order.iter().position(|v| *v == self)?;
        super::focus_step(code, index, if reply { 5 } else { 4 }).map(|i| order[i])
    }

    fn action(self, selected: Option<&str>) -> Option<BoardAction<'static>> {
        match self {
            Self::Reply => selected.map(|id| BoardAction::QuickReply(id.to_owned())),
            Self::List => None,
            Self::Compose => Some(BoardAction::Compose),
            Self::Preferences => Some(BoardAction::Preferences),
            Self::Workspace => Some(BoardAction::Workspace),
        }
    }
}

const BUTTONS: [(Focus, &str); 4] = [
    (Focus::Compose, "写信"),
    (Focus::Preferences, "偏好"),
    (Focus::Workspace, "工作区"),
    (Focus::Reply, "快速回复"),
];

fn layout(area: Rect, reply: bool) -> (Rect, std::rc::Rc<[Rect]>) {
    let count = if reply { 4 } else { 3 };
    let [list, bar] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    (
        list,
        Layout::horizontal(vec![Constraint::Ratio(1, count); count as usize]).split(bar),
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
    fn selected_letter(&self) -> Option<&str> {
        match &self.screen {
            BoardScreen::Board(key) => key.strip_prefix("letter:"),
            _ => None,
        }
    }
    pub fn render(&mut self, frame: &mut Frame<'_>, rows: &BoardRows<'_>) {
        self.render_with_theme(frame, rows, super::preferences::ThemeColor::default());
    }

    pub fn render_with_theme(
        &mut self,
        frame: &mut Frame<'_>,
        rows: &BoardRows<'_>,
        color: super::preferences::ThemeColor,
    ) {
        let views = rows.views();
        self.screen.refresh(&views);
        if matches!(self.screen, BoardScreen::Detail(_)) {
            self.screen.render(frame, &views);
            return;
        }
        let (list, buttons) = layout(frame.area(), self.selected_letter().is_some());
        self.screen.render_area(frame, list, &views, color);
        for ((focus, label), area) in BUTTONS.into_iter().zip(buttons.iter().copied()) {
            let widget = Paragraph::new(label).centered();
            let widget = if focus == self.focus {
                widget.style(color.highlight())
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
                } else if self.focus == Focus::List
                    && self.screen.position(&views).is_some_and(|index| {
                        (key.code == KeyCode::Up && index > 0)
                            || (key.code == KeyCode::Down && index + 1 < views.len())
                    })
                {
                    self.screen.navigate(key.code, &views);
                } else if let Some(next) = self
                    .focus
                    .move_focus(key.code, self.selected_letter().is_some())
                {
                    self.focus = next;
                } else if key.code == KeyCode::Enter && self.focus != Focus::List {
                    return self.focus.action(self.selected_letter());
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
                let (list, buttons) = layout(area, self.selected_letter().is_some());
                if list.contains(point) {
                    self.focus = Focus::List;
                }
                for ((focus, _), button) in BUTTONS.into_iter().zip(buttons.iter().copied()) {
                    if button.contains(point) {
                        self.focus = focus;
                        return focus.action(self.selected_letter());
                    }
                }
            }
            _ => {}
        }
        None
    }
}
