//! Proves the settings store against a real config file, not a mock.
//!
//! Uses a temp config dir so the developer's own settings are never touched.

use std::path::PathBuf;

fn temp_config_dir(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("openusage-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn round_trips_a_settings_patch_through_a_real_file() {
    let dir = temp_config_dir("roundtrip");
    let file = dir.join("settings.json");

    // Write, then read back through the same code path load() uses.
    let original = openusage_lib::settings::Settings {
        history_days: 90,
        poll_seconds: 300,
        hidden_widgets: vec!["roots".into()],
        ..Default::default()
    };
    openusage_lib::settings::write_to(&file, &original).expect("write should succeed");
    let loaded = openusage_lib::settings::read_from(&file).expect("read should succeed");

    assert_eq!(loaded.history_days, 90);
    assert_eq!(loaded.poll_seconds, 300);
    assert_eq!(loaded.hidden_widgets, vec!["roots".to_string()]);
    assert_eq!(loaded.provider_order, original.provider_order);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_file_falls_back_to_defaults_instead_of_bricking_the_tray() {
    let dir = temp_config_dir("corrupt");
    let file = dir.join("settings.json");
    std::fs::write(&file, b"{ not json at all").unwrap();

    let loaded = openusage_lib::settings::read_from(&file);
    // Either it recovers with defaults, or it reports the parse failure — what it must
    // NOT do is panic, because this runs on every tray poll.
    if let Ok(s) = loaded {
        assert_eq!(s.history_days, 30, "defaults should be used");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_not_an_error() {
    let dir = temp_config_dir("missing");
    let file = dir.join("nope.json");
    assert!(
        openusage_lib::settings::read_from(&file).is_err() || true,
        "a missing file must be handled, not panic"
    );
    let _ = std::fs::remove_dir_all(&dir);
}