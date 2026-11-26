use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use synod::tui::{
    LetterView,
    bulletin::{BoardAction, BoardRows, Bulletin, DraftView},
};

#[test]
fn draft_precedes_publication_and_buttons_share_actions() -> Result<(), std::convert::Infallible> {
    let drafts = [DraftView {
        reference: "drafts/a/x/primary",
        title: "Private",
    }];
    let letters = [LetterView {
        id: "x",
        title: "Published",
        body: "Body",
    }];
    let rows = BoardRows::new(&drafts, &letters);
    let area = Rect::new(0, 0, 60, 8);
    let mut board = Bulletin::default();
    let mut terminal = Terminal::new(TestBackend::new(60, 8))?;
    let key = |code: KeyCode| Event::Key(code.into());
    let click = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    terminal.draw(|f| board.render(f, &rows))?;
    let buffer = terminal.backend().buffer();
    let line = |row| {
        (0..60)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
    };
    assert!(line(2).contains("Private") && line(2).contains("草"));
    assert!(line(3).contains("Published"));
    assert_eq!(
        board.handle(key(KeyCode::Enter), area, &rows),
        Some(BoardAction::OpenDraft(drafts[0].reference))
    );
    for (x, expected) in [
        (5, BoardAction::Compose),
        (15, BoardAction::Workspace),
        (25, BoardAction::Filter),
        (35, BoardAction::Preferences),
    ] {
        board.handle(key(KeyCode::Tab), area, &rows);
        terminal.draw(|f| board.render(f, &rows))?;
        assert_eq!(
            board.handle(key(KeyCode::Enter), area, &rows).as_ref(),
            Some(&expected)
        );
        assert_eq!(
            board.handle(click(x, 6), area, &rows).as_ref(),
            Some(&expected)
        );
    }
    board.handle(key(KeyCode::Tab), area, &rows);
    board.handle(click(2, 2), area, &rows);
    board.handle(key(KeyCode::Down), area, &rows);
    board.handle(key(KeyCode::Enter), area, &rows);
    terminal.draw(|f| board.render(f, &rows))?;
    assert_eq!(board.handle(click(50, 7), area, &rows), None);
    board.handle(key(KeyCode::Esc), area, &rows);
    assert_eq!(
        board.handle(key(KeyCode::Enter), area, &BoardRows::new(&[], &[])),
        None
    );
    board.handle(key(KeyCode::Tab), area, &rows);
    assert_eq!(board.handle(key(KeyCode::Down), area, &rows), None);
    assert_eq!(board.handle(click(61, 9), area, &rows), None);
    assert_eq!(board.handle(Event::Resize(80, 24), area, &rows), None);
    Ok(())
}

#[test]
fn quick_reply_targets_the_selected_published_letter() {
    let letters = [LetterView {
        id: "letter-18",
        title: "Figure",
        body: "Body",
    }];
    let rows = BoardRows::new(&[], &letters);
    let mut board = Bulletin::default();
    let area = Rect::new(0, 0, 60, 8);
    assert_eq!(
        board.handle(Event::Key(KeyCode::Char('r').into()), area, &rows),
        Some(BoardAction::QuickReply("letter-18".into()))
    );
}

#[test]
fn arrows_reach_actions_from_both_ends_of_the_list() {
    let rows = BoardRows::new(&[], &[]);
    let area = Rect::new(0, 0, 60, 8);
    let mut board = Bulletin::default();
    for direction in [KeyCode::Left, KeyCode::Up, KeyCode::Right] {
        board.handle(Event::Key(direction.into()), area, &rows);
        assert_eq!(
            board.handle(Event::Key(KeyCode::Enter.into()), area, &rows),
            None
        );
    }
    for (direction, expected) in [
        (KeyCode::Down, BoardAction::Filter),
        (KeyCode::Left, BoardAction::Workspace),
        (KeyCode::Left, BoardAction::Compose),
        (KeyCode::Right, BoardAction::Workspace),
        (KeyCode::Right, BoardAction::Filter),
        (KeyCode::Right, BoardAction::Preferences),
    ] {
        board.handle(Event::Key(direction.into()), area, &rows);
        assert_eq!(
            board.handle(Event::Key(KeyCode::Enter.into()), area, &rows),
            Some(expected)
        );
    }
}
