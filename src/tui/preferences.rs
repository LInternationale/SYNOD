//! Editing state for the custom RGB theme preference.

/// Three validated 8-bit color channels in red, green, blue order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeColor(pub [u8; 3]);

impl Default for ThemeColor {
    fn default() -> Self {
        Self(crate::DEFAULT_THEME_RGB)
    }
}

impl ThemeColor {
    pub(super) fn highlight(self) -> Style {
        let [r, g, b] = self.0;
        Style::default().bg(Color::Rgb(r, g, b))
    }
}

/// Editable input and the currently applied preference.
pub struct ThemeEditor {
    active: ThemeColor,
    pub channels: [String; 3],
}

/// The field needing correction or the persistence error returned by the client.
#[derive(Debug, Eq, PartialEq)]
pub enum ApplyError<E> {
    InvalidChannel(usize),
    Save(E),
}

impl ThemeEditor {
    pub fn new(active: ThemeColor) -> Self {
        Self {
            active,
            channels: active.0.map(|value| value.to_string()),
        }
    }

    pub fn active(&self) -> ThemeColor {
        self.active
    }

    /// Apply valid input once local preferences have been saved.
    ///
    /// ```
    /// use synod::tui::preferences::{ThemeColor, ThemeEditor};
    /// let mut editor = ThemeEditor::new(ThemeColor([0, 0, 0]));
    /// editor.channels[0] = "255".into();
    /// editor.apply(|_| Ok::<_, ()>(())).unwrap();
    /// assert_eq!(editor.active(), ThemeColor([255, 0, 0]));
    /// ```
    pub fn apply<E>(
        &mut self,
        save: impl FnOnce(ThemeColor) -> Result<(), E>,
    ) -> Result<(), ApplyError<E>> {
        let mut rgb = [0; 3];
        for (index, channel) in self.channels.iter().enumerate() {
            rgb[index] = channel
                .parse()
                .map_err(|_| ApplyError::InvalidChannel(index))?;
        }
        let color = ThemeColor(rgb);
        save(color).map_err(ApplyError::Save)?;
        self.active = color;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_and_failed_storage_preserve_edits_and_active_color() {
        let initial = ThemeColor([1, 2, 3]);
        let mut editor = ThemeEditor::new(initial);
        editor.channels[2] = "256".into();
        assert_eq!(
            editor.apply(|_| Err(())),
            Err(ApplyError::InvalidChannel(2))
        );
        editor.channels[2] = "0".into();
        assert_eq!(editor.apply(|_| Err("disk")), Err(ApplyError::Save("disk")));
        assert_eq!(editor.active(), initial);
        assert_eq!(editor.channels[2], "0");
        assert_eq!(editor.apply(|_| Ok::<_, ()>(())), Ok(()));
        assert_eq!(editor.active(), ThemeColor([1, 2, 0]));
    }
}

use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{Block, Paragraph},
};

/// The owning TUI routes Back to the board.
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Stay,
    Back,
}

/// Interactive custom RGB preference form.
pub struct ThemeForm {
    pub editor: ThemeEditor,
    focus: usize,
    replace: bool,
}

impl ThemeForm {
    pub fn new(active: ThemeColor) -> Self {
        Self {
            editor: ThemeEditor::new(active),
            focus: 0,
            replace: true,
        }
    }

    fn areas(area: Rect) -> [Rect; 4] {
        Layout::vertical([Constraint::Length(1); 4]).areas(Block::bordered().inner(area))
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Block::bordered().title("偏好 · 设置主题色"), area);
        let [r, g, b] = &self.editor.channels;
        let labels = [
            format!("R  {r}"),
            format!("G  {g}"),
            format!("B  {b}"),
            String::from("应用设置"),
        ];
        for (index, (label, rect)) in labels.into_iter().zip(Self::areas(area)).enumerate() {
            let line = Paragraph::new(label);
            frame.render_widget(
                if index == self.focus {
                    line.style(self.editor.active().highlight())
                } else {
                    line
                },
                rect,
            );
        }
    }

    pub fn handle<E>(
        &mut self,
        event: Event,
        area: Rect,
        save: impl FnOnce(ThemeColor) -> Result<(), E>,
    ) -> Result<SettingsAction, ApplyError<E>> {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) = Self::areas(area)
                    .iter()
                    .position(|rect| rect.contains(Position::new(mouse.column, mouse.row)))
                {
                    self.focus = index;
                    self.replace = true;
                    if index == 3 {
                        self.editor.apply(save)?;
                    }
                }
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(SettingsAction::Back),
                KeyCode::Tab => {
                    self.focus = (self.focus + 1) % 4;
                    self.replace = true;
                }
                KeyCode::Enter if self.focus == 3 => self.editor.apply(save)?,
                KeyCode::Char(c) if self.focus < 3 && c.is_ascii_digit() => {
                    let value = &mut self.editor.channels[self.focus];
                    if self.replace {
                        value.clear();
                        self.replace = false;
                    }
                    value.push(c);
                }
                KeyCode::Backspace if self.focus < 3 => {
                    self.editor.channels[self.focus].pop();
                    self.replace = false;
                }
                _ => {}
            },
            _ => {}
        }
        Ok(SettingsAction::Stay)
    }
}
