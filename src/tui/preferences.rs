//! Editing state for the custom RGB theme preference.

/// Three validated 8-bit color channels in red, green, blue order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeColor(pub [u8; 3]);

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
