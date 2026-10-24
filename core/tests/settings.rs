use cvc_core::settings::Settings;

#[test]
fn settings_saved_by_the_dotnet_version_are_kept() {
    // The .NET version wrote System.Text.Json defaults (PascalCase) to the same settings.json.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"AutoCopy":false,"DelaySeconds":5.5}"#).unwrap();

    let loaded = Settings::load(&path);
    assert!(!loaded.auto_copy);
    assert_eq!(5.5, loaded.delay_seconds);
    assert!(!loaded.risk_notice_seen);

    loaded.save(&path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"autoCopy\": false"), "{saved}");
    assert_eq!(loaded, Settings::load(&path));
}
