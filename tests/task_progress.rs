use synod::application::letters::{PublishedLetter, project_progress};
fn node(id: &str, children: &[&str]) -> PublishedLetter {
    PublishedLetter {
        id: id.into(),
        title: id.into(),
        author: String::new(),
        to: String::new(),
        cc: vec![],
        references: vec![],
        dependencies: children.iter().map(|s| s.to_string()).collect(),
        progress: 0,
        progress_done: 0,
        progress_total: 0,
        completed: false,
        unread: false,
        checklist: vec![],
        events: vec![],
        body: String::new(),
    }
}
#[test]
fn counts_self_and_shared_descendants_once_without_changing_manual_flags() -> std::io::Result<()> {
    let mut graph = [
        node("A", &["B", "C"]),
        node("B", &["D", "E"]),
        node("C", &["F"]),
        node("D", &[]),
        node("E", &[]),
        node("F", &[]),
    ];
    graph[1].completed = true;
    project_progress(&mut graph)?;
    assert_eq!((graph[0].progress_done, graph[0].progress_total), (3, 6));
    assert_eq!((graph[2].progress_done, graph[2].progress_total), (0, 2));
    graph[2].dependencies.push("D".into());
    project_progress(&mut graph)?;
    assert_eq!((graph[0].progress_done, graph[0].progress_total), (3, 6));
    graph[2].completed = true;
    project_progress(&mut graph)?;
    assert_eq!(graph[0].progress_done, 5);
    assert!(!graph[3].completed);
    graph[1].completed = false;
    project_progress(&mut graph)?;
    assert_eq!(graph[0].progress_done, 3);
    graph[0].completed = true;
    project_progress(&mut graph)?;
    assert_eq!(graph[0].progress, 100);
    assert_eq!(graph[3].progress_done, 0);
    assert_eq!(graph[3].progress_total, 1);
    graph[5].dependencies.push("A".into());
    assert!(project_progress(&mut graph).is_err());
    Ok(())
}

#[test]
fn shared_inline_items_keep_their_states_under_manual_coverage() -> std::io::Result<()> {
    let mut graph = [node("A", &["B", "C"]), node("B", &["C"]), node("C", &[])];
    graph[2].checklist = synod::application::letters::checklist(
        "- [ ] same\n  - [X] same\n```md\n- [ ] example\n```",
    );
    assert_eq!(graph[2].checklist.len(), 2);
    for (covered, expected) in [(false, 1), (true, 4), (false, 1)] {
        graph[1].completed = covered;
        project_progress(&mut graph)?;
        assert_eq!(
            (graph[0].progress_done, graph[0].progress_total),
            (expected, 5)
        );
        assert_eq!(graph[2].progress_done, 1);
    }
    Ok(())
}
