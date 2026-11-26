//! Letter title comparison within a team's published bulletin board.

/// A borrowed comparison key for a letter title.
///
/// ```
/// use synod::mail::letter::TitleKey;
/// assert_eq!(TitleKey::from(" Figure 3 "), TitleKey::from("Figure 3"));
/// ```
#[derive(Debug, Eq, Hash, PartialEq)]
pub struct TitleKey<'a>(&'a str);

impl<'a> From<&'a str> for TitleKey<'a> {
    fn from(title: &'a str) -> Self {
        Self(title.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::TitleKey;

    #[test]
    fn trims_unicode_edges_only() {
        assert_eq!(
            TitleKey::from("\u{3000}Figure 3\n"),
            TitleKey::from("Figure 3")
        );
        assert_ne!(TitleKey::from("Figure  3"), TitleKey::from("Figure 3"));
        assert_ne!(TitleKey::from("figure 3"), TitleKey::from("Figure 3"));
        assert_ne!(TitleKey::from("é"), TitleKey::from("e\u{301}"));
    }
}
