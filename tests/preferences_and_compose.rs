use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use synod::tui::{
    compose::{Editor, Exit},
    preferences::{SettingsAction, ThemeColor, ThemeForm},
};

#[test]
fn form_applies_only_on_keyboard_or_mouse_activation() -> Result<(), std::convert::Infallible> {
    let initial = ThemeColor::default();
    assert_eq!(initial.0, [154, 176, 194]);
    let mut form = ThemeForm::new(initial);
    let area = Rect::new(0, 0, 32, 8);
    let mut terminal = Terminal::new(TestBackend::new(32, 8))?;
    terminal.draw(|f| form.render(f))?;
    let mut saved = Vec::new();
    let mut save = |c| {
        saved.push(c);
        Ok::<_, ()>(())
    };
    for code in [
        KeyCode::Char('2'),
        KeyCode::Char('3'),
        KeyCode::Backspace,
        KeyCode::Char('1'),
        KeyCode::Tab,
        KeyCode::Tab,
        KeyCode::Tab,
    ] {
        assert_eq!(
            form.handle(Event::Key(code.into()), area, &mut save),
            Ok(SettingsAction::Stay)
        );
    }
    assert_eq!(form.editor.active(), initial);
    assert_eq!(
        form.handle(Event::Key(KeyCode::Enter.into()), area, &mut save),
        Ok(SettingsAction::Stay)
    );
    for row in [3, 6] {
        let event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            form.handle(event, area, &mut save),
            Ok(SettingsAction::Stay)
        );
    }
    assert_eq!(
        form.handle(Event::Key(KeyCode::Esc.into()), area, &mut save),
        Ok(SettingsAction::Back)
    );
    assert_eq!(saved, [ThemeColor([21, 176, 194]); 2]);
    Ok(())
}

#[test]
fn failed_save_preserves_changes_and_reverting_to_saved_exits_cleanly() {
    let mut editor = Editor::open(String::from("first"));
    editor.current.push('!');
    assert_eq!(editor.save(|_| Err("disk")), Err("disk"));
    let Exit::ConfirmDiscard(mut editor) = editor.escape() else {
        panic!("expected confirmation")
    };
    assert_eq!(editor.current, "first!");
    assert!(editor.save(|_| Ok::<_, ()>(())).is_ok());
    editor.current.push('?');
    editor.current.pop();
    assert!(matches!(editor.escape(), Exit::Board));
}

#[test]
fn system_preferences_roundtrip_preserves_other_keys() -> std::io::Result<()> {
    use synod::adapters::local::state::ClientConfig;
    let root = std::env::temp_dir().join(format!("synod-config-test-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    let path = root.join("client.json");
    assert_eq!(ClientConfig::read(&path)?.theme_rgb, [154, 176, 194]);
    std::fs::write(&path, br#"{"existing":"keep"}"#)?;
    let mut config = ClientConfig::read(&path)?;
    assert_eq!(config.theme_rgb, [154, 176, 194]);
    config.theme_rgb = [5, 6, 7];
    config.save(&path)?;
    let loaded = ClientConfig::read(&path)?;
    assert_eq!(loaded.fields["existing"], "keep");
    assert_eq!(loaded.theme_rgb, [5, 6, 7]);
    std::fs::write(&path, br#"{"theme_rgb":[70,93,112]}"#)?;
    assert_eq!(ClientConfig::read(&path)?.theme_rgb, [154, 176, 194]);
    std::fs::write(&path, br#"{"theme_rgb":[70,93,112],"custom_theme":true}"#)?;
    assert_eq!(ClientConfig::read(&path)?.theme_rgb, [70, 93, 112]);
    std::fs::write(&path, b"{")?;
    assert!(ClientConfig::read(&path).is_err());
    std::fs::remove_file(path)?;
    std::fs::remove_dir(root)
}
