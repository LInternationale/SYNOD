//! Bulletin board navigation and rendering.

use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    widgets::{Block, List, ListItem, Paragraph, Wrap},
};

/// Published letter data supplied by the session.
pub struct LetterView<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub body: &'a str,
}

/// Board selection or an open detail identified across refreshes.
///
/// ```
/// use synod::tui::{BoardScreen, LetterView};
/// use crossterm::event::KeyCode;
/// let letters = [LetterView { id: "one", title: "Delivery", body: "Ready" }];
/// let mut screen = BoardScreen::default();
/// screen.navigate(KeyCode::Enter, &letters);
/// assert!(matches!(screen, BoardScreen::Detail(ref id) if id == "one"));
/// ```
#[derive(Default)]
pub enum BoardScreen {
    #[default]
    Empty,
    Board(String),
    Detail(String),
}

impl BoardScreen {
    /// Reconcile selection before displaying a refreshed session view.
    pub fn refresh(&mut self, letters: &[LetterView<'_>]) {
        if self.position(letters).is_none() {
            *self = letters
                .first()
                .map_or(Self::Empty, |letter| Self::Board(letter.id.into()));
        }
    }

    fn position(&self, letters: &[LetterView<'_>]) -> Option<usize> {
        let id = match self {
            Self::Empty => return None,
            Self::Board(id) | Self::Detail(id) => id,
        };
        letters.iter().position(|letter| letter.id == id)
    }

    /// Apply a navigation key to the current session view.
    pub fn navigate(&mut self, key: KeyCode, letters: &[LetterView<'_>]) {
        self.refresh(letters);
        let Some(index) = self.position(letters) else {
            return;
        };
        match (&self, key) {
            (Self::Board(_), KeyCode::Enter) => *self = Self::Detail(letters[index].id.into()),
            (Self::Detail(_), KeyCode::Esc) => *self = Self::Board(letters[index].id.into()),
            (Self::Board(_), KeyCode::Up) => {
                *self = Self::Board(letters[index.saturating_sub(1)].id.into())
            }
            (Self::Board(_), KeyCode::Down) => {
                *self = Self::Board(letters[(index + 1).min(letters.len() - 1)].id.into())
            }
            _ => {}
        }
    }

    /// Draw the current screen into a Ratatui frame.
    pub fn render(&mut self, frame: &mut Frame<'_>, letters: &[LetterView<'_>]) {
        self.render_area(
            frame,
            frame.area(),
            letters,
            preferences::ThemeColor::default(),
        );
    }

    fn render_area(
        &mut self,
        frame: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        letters: &[LetterView<'_>],
        color: preferences::ThemeColor,
    ) {
        self.refresh(letters);
        let selection = self.position(letters);
        if let (Self::Detail(_), Some(index)) = (&self, selection) {
            let letter = &letters[index];
            frame.render_widget(
                Paragraph::new(letter.body)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(letter.title)),
                area,
            );
            return;
        }
        let items = letters.iter().enumerate().map(|(index, letter)| {
            let item = ListItem::new(letter.title);
            if Some(index) == selection {
                item.style(color.highlight())
            } else {
                item
            }
        });
        let mut state = ratatui::widgets::ListState::default().with_selected(selection);
        frame.render_stateful_widget(
            List::new(items).block(Block::bordered().title("公告板")),
            area,
            &mut state,
        );
    }
}

pub mod bulletin;
pub mod compose;
pub mod preferences;

#[cfg(unix)]
pub mod live;
