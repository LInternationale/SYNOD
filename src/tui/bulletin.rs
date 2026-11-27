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
    Read(String),
    Queued(String),
    Filter,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    List,
    Compose,
    Preferences,
    Workspace,
    Filter,
}

impl Focus {
    fn move_focus(self, code: KeyCode, reply: bool, area: Rect) -> Option<Self> {
        let order = [
            Self::List,
            Self::Compose,
            Self::Workspace,
            Self::Filter,
            Self::Preferences,
        ];
        let index = order.iter().position(|v| *v == self)?;
        {
            let (list, buttons) = layout(area, reply);
            let controls = std::iter::once(list)
                .chain(buttons.iter().copied())
                .enumerate()
                .collect::<Vec<_>>();
            super::focus_step(code, index, controls.len(), &controls)
        }
        .map(|i| order[i])
    }

    fn action(self, _selected: Option<&str>) -> Option<BoardAction<'static>> {
        match self {
            Self::Filter => Some(BoardAction::Filter),
            Self::List => None,
            Self::Compose => Some(BoardAction::Compose),
            Self::Preferences => Some(BoardAction::Preferences),
            Self::Workspace => Some(BoardAction::Workspace),
        }
    }
}

const BUTTONS: [(Focus, &str); 4] = [
    (Focus::Compose, "写信"),
    (Focus::Workspace, "工作区"),
    (Focus::Filter, "筛选"),
    (Focus::Preferences, "偏好"),
];
fn layout(area: Rect, _reply: bool) -> (Rect, Vec<Rect>) {
    let [list, bar, _] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(super::action_height(area.width, 4)),
        Constraint::Length(1),
    ])
    .areas(area);
    (list, super::action_cells(bar, 4))
}

/// Session snapshot projected into display rows. Keys are isolated by source.
pub struct BoardRows<'a> {
    drafts: &'a [DraftView<'a>],
    rows: Vec<(String, String, &'a str)>,
    queued: usize,
}

impl<'a> BoardRows<'a> {
    pub fn new(drafts: &'a [DraftView<'a>], published: &[LetterView<'a>]) -> Self {
        let rows = drafts
            .iter()
            .map(|d| {
                (
                    format!("draft:{}", d.reference),
                    format!("# {}", d.title),
                    "",
                )
            })
            .chain(
                published
                    .iter()
                    .map(|l| (format!("letter:{}", l.id), l.title.into(), l.body)),
            )
            .collect();
        Self {
            drafts,
            rows,
            queued: 0,
        }
    }

    pub(super) fn queued(
        mut self,
        queue: &[&crate::session::protocol::DeliveryRow],
        spinner: &str,
    ) -> Self {
        use crate::session::protocol::DeliveryState;
        let mut prefix = Vec::new();
        for job in queue {
            let key = format!("letter:{}", job.letter);
            if self.rows.iter().any(|r| r.0 == key) {
                continue;
            }
            prefix.push((
                key,
                format!(
                    "{} {}",
                    if matches!(job.state, DeliveryState::Failed(..)) {
                        "!"
                    } else {
                        spinner
                    },
                    job.title
                ),
                "",
            ));
        }
        self.queued = prefix.len();
        prefix.append(&mut self.rows);
        self.rows = prefix;
        self
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
    pub(super) fn last_action(&self) -> bool {
        self.focus == Focus::Preferences
    }
    pub fn return_to_list(&mut self) {
        if let BoardScreen::Detail(id) = &self.screen {
            self.screen = BoardScreen::Board(id.clone());
        }
    }
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
            self.screen
                .render_area(frame, frame.area(), &views, color, true);
            return;
        }
        let (list, buttons) = layout(frame.area(), self.selected_letter().is_some());
        self.screen
            .render_area(frame, list, &views, color, self.focus == Focus::List);
        for ((focus, label), area) in BUTTONS.into_iter().zip(buttons.iter().copied()) {
            let ordinal = BUTTONS
                .iter()
                .position(|b| b.0 == focus)
                .unwrap_or_default()
                + 1;
            let widget = Paragraph::new(format!("F{ordinal} {label}")).centered();
            let widget = if focus == self.focus {
                widget.style(color.highlight())
            } else {
                widget
            };
            frame.render_widget(widget, area);
        }
        let pending = self
            .screen
            .position(&views)
            .is_some_and(|index| index < rows.queued);
        let hint = match self.focus {
            Focus::List if pending => "enter · 查看",
            Focus::List if self.selected_letter().is_some() => "enter · 查看信件    r · 快速回复",
            Focus::List if !views.is_empty() => "enter · 编辑草稿",
            Focus::List => "",
            _ => "enter · 打开",
        };
        let area = frame.area();
        frame.render_widget(
            Paragraph::new(hint),
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
        );
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
                if !matches!(self.screen, BoardScreen::Detail(_)) {
                    if let Some(i) = super::shortcut(key, 4, false) {
                        self.focus = BUTTONS[i].0;
                        return self.focus.action(self.selected_letter());
                    }
                    if self.focus == Focus::List
                        && key.code == KeyCode::Char('r')
                        && !self
                            .screen
                            .position(&views)
                            .is_some_and(|i| i < rows.queued)
                    {
                        return self
                            .selected_letter()
                            .map(|id| BoardAction::QuickReply(id.into()));
                    }
                }
                if matches!(self.screen, BoardScreen::Detail(_)) {
                    self.screen.navigate(key.code, &views);
                } else if self.focus == Focus::List
                    && self.screen.position(&views).is_some_and(|index| {
                        (key.code == KeyCode::Up && index > 0)
                            || (key.code == KeyCode::Down && index + 1 < views.len())
                    })
                {
                    self.screen.navigate(key.code, &views);
                } else if let Some(next) =
                    self.focus
                        .move_focus(key.code, self.selected_letter().is_some(), area)
                {
                    self.focus = next;
                } else if key.code == KeyCode::Enter && self.focus != Focus::List {
                    return self.focus.action(self.selected_letter());
                } else if self.focus == Focus::List {
                    if key.code == KeyCode::Enter {
                        if self
                            .screen
                            .position(&views)
                            .is_some_and(|i| i < rows.queued)
                        {
                            return Some(BoardAction::Queued(self.selected_letter()?.to_owned()));
                        }
                        if let Some(draft) = self
                            .screen
                            .position(&views)
                            .and_then(|i| rows.drafts.get(i.checked_sub(rows.queued)?))
                        {
                            return Some(BoardAction::OpenDraft(draft.reference));
                        }
                    }
                    self.screen.navigate(key.code, &views);
                    if key.code == KeyCode::Enter
                        && let BoardScreen::Detail(key) = &self.screen
                        && let Some(id) = key.strip_prefix("letter:")
                    {
                        return Some(BoardAction::Read(id.into()));
                    }
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
