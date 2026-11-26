use serde::Deserialize;
use synod::adapters::git::commit_message::{DecodeError, decode};

#[derive(Deserialize)]
struct History {
    step: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    message: String,
}

#[test]
fn reads_fixture_and_rejects_incompatible_messages() -> Result<(), toml::de::Error> {
    let history: History = toml::from_str(include_str!("fixtures/v1/history.toml"))?;
    for step in &history.step {
        assert!(decode(&step.message).is_ok());
        assert!(matches!(
            decode(
                &step
                    .message
                    .replace("schema_version = 1", "schema_version = 2")
            ),
            Err(DecodeError::Version(2))
        ));
        assert!(matches!(
            decode(&step.message.replace("+08:00", "")),
            Err(DecodeError::Timestamp)
        ));
        assert!(matches!(
            decode(&step.message.replace('\n', "\r\n")),
            Err(DecodeError::LineEnding)
        ));
        assert!(matches!(
            decode(step.message.trim_end()),
            Err(DecodeError::LineEnding)
        ));
        assert!(matches!(
            decode(&step.message.replace("team_id =", "invalid =")),
            Err(DecodeError::Toml(_))
        ));
    }
    let first = &history.step[0].message;
    assert!(matches!(
        decode(&first.replace("team.configured", "new.kind")),
        Err(DecodeError::UnknownKind(_))
    ));
    assert!(matches!(
        decode(&first.replace("team.configured", "letter.sent")),
        Err(DecodeError::Target)
    ));
    assert!(matches!(
        decode(&format!("{first}[target]\n")),
        Err(DecodeError::Target)
    ));
    for step in &history.step[1..] {
        assert!(matches!(
            decode(&step.message.replace("[target]", "[target]\noops = 5")),
            Err(DecodeError::Toml(_))
        ));
    }
    Ok(())
}
