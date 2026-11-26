//! Labels for the visible paths in a flat, keyboard-navigable tree.
use std::collections::HashMap;
use std::path::Path;

pub(super) fn labels(names: &mut [String]) -> Vec<String> {
    names.sort_by(|a, b| Path::new(a).components().cmp(Path::new(b).components()));
    let mut last = HashMap::new();
    for name in names.iter() {
        let path = Path::new(name);
        last.insert(path.parent(), path);
    }
    names
        .iter()
        .map(|name| {
            let path = Path::new(name);
            let mut branches = path
                .ancestors()
                .filter(|p| !p.as_os_str().is_empty())
                .collect::<Vec<_>>();
            branches.reverse();
            let mut text = String::new();
            for branch in branches {
                let final_child = last.get(&branch.parent()) == Some(&branch);
                text.push_str(match (branch == path, final_child) {
                    (true, true) => "└─ ",
                    (true, false) => "├─ ",
                    (false, true) => "   ",
                    (false, false) => "│  ",
                });
            }
            text.push_str(path.file_name().and_then(|s| s.to_str()).unwrap_or(name));
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_prefixes_remain_siblings() {
        let mut paths = ["a.txt", "a/n/x", "a", "b", "a/n"].map(str::to_owned);
        assert_eq!(
            super::labels(&mut paths),
            ["├─ a", "│  └─ n", "│     └─ x", "├─ a.txt", "└─ b"]
        );
    }
}
