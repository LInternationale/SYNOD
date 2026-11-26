use std::collections::HashSet;
use synod::mail::letter::TitleKey;

#[test]
fn published_titles_can_be_looked_up_by_trimmed_spelling() {
    let titles = HashSet::from([TitleKey::from(" Figure 3")]);
    assert!(titles.contains(&TitleKey::from("Figure 3 ")));
}
