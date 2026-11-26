use synod::application::setup::{NewTeam, SetupError};

#[test]
fn wizard_data_becomes_a_v1_manifest() -> Result<(), SetupError> {
    let names = || ["甲", "乙", "丙"].map(str::to_owned);
    let manifest = NewTeam::from_wizard("team-a", names(), names(), vec!["论文".into()])?;
    let value = serde_json::to_value(manifest).expect("serializable manifest");
    assert_eq!(value["members"][0]["display_name"], "甲");
    assert_eq!(value["members"][5]["kind"], "assistant");
    assert_eq!(value["spaces"][0]["display_name"], "论文");
    assert!(matches!(
        NewTeam::from_wizard("../a", names(), names(), vec![]),
        Err(SetupError::InvalidTeamId)
    ));
    let blank = || ["a", " ", "b"].map(str::to_owned);
    assert!(matches!(
        NewTeam::from_wizard("t", blank(), names(), vec![]),
        Err(SetupError::EmptyHumanName(1))
    ));
    assert!(matches!(
        NewTeam::from_wizard("t", names(), blank(), vec![]),
        Err(SetupError::EmptyAssistantName(1))
    ));
    assert!(matches!(
        NewTeam::from_wizard("t", names(), names(), vec![String::new()]),
        Err(SetupError::EmptySpaceName(0))
    ));
    Ok(())
}
