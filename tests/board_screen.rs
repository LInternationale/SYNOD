use crossterm::event::KeyCode;
use ratatui::{Terminal, backend::TestBackend};
use synod::tui::{BoardScreen, LetterView};

#[test]
fn selected_letter_survives_view_changes() -> Result<(), std::convert::Infallible> {
    let mut screen = BoardScreen::default();
    let mut letters = [
        LetterView {
            id: "a",
            title: "First",
            body: "Alpha",
        },
        LetterView {
            id: "b",
            title: "Second",
            body: "Bravo",
        },
    ];
    let mut terminal = Terminal::new(TestBackend::new(24, 6))?;
    screen.navigate(KeyCode::Enter, &[]);
    for key in [KeyCode::Up, KeyCode::Down, KeyCode::Down, KeyCode::Enter] {
        screen.navigate(key, &letters);
    }
    letters.reverse();
    screen.navigate(KeyCode::Down, &letters);
    terminal.draw(|frame| screen.render(frame, &letters))?;
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("Bravo")
    );
    screen.navigate(KeyCode::Esc, &letters);
    assert!(matches!(&screen, BoardScreen::Board(id) if id == "b"));
    terminal.draw(|frame| screen.render(frame, &letters))?;
    screen.refresh(&[]);
    assert!(matches!(screen, BoardScreen::Empty));
    Ok(())
}
