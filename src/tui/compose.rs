//! Draft editor lifecycle shared by keyboard and mouse actions.

/// The original and current editable letter data.
pub struct Editor<T> {
    pub current: T,
    saved: T,
}

/// Escape either closes a clean editor or retains unsaved edits for confirmation.
pub enum Exit<T> {
    Board,
    ConfirmDiscard(Editor<T>),
}

impl<T: Clone + Eq> Editor<T> {
    /// Open a new or existing draft with its last known contents.
    pub fn open(snapshot: T) -> Self {
        Self {
            current: snapshot.clone(),
            saved: snapshot,
        }
    }

    /// Acknowledge the snapshot only after local persistence succeeds.
    pub fn save<E>(&mut self, persist: impl FnOnce(&T) -> Result<(), E>) -> Result<(), E> {
        persist(&self.current)?;
        self.saved.clone_from(&self.current);
        Ok(())
    }

    /// Evaluate Escape against the latest acknowledged save.
    pub fn escape(self) -> Exit<T> {
        if self.current == self.saved {
            Exit::Board
        } else {
            Exit::ConfirmDiscard(self)
        }
    }
}
