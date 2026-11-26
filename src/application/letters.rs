//! Letter composition and publication views.
use serde::{Deserialize, Serialize};
#[derive(Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub reference: String,
    pub title: String,
    pub to: String,
    pub cc: String,
    pub body: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedLetter {
    pub id: String,
    pub title: String,
    pub author: String,
    pub body: String,
}

impl Draft {
    pub fn replying_to(letter: &PublishedLetter, board: &[PublishedLetter]) -> Self {
        let base = format!("Re: {}", letter.title);
        let mut title = base.clone();
        let mut suffix = 2;
        while board.iter().any(|item| item.title.trim() == title.trim()) {
            title = format!("{base} - {suffix}");
            suffix += 1;
        }
        Self {
            title,
            to: letter.author.clone(),
            references: vec![letter.id.clone()],
            ..Self::default()
        }
    }
}
