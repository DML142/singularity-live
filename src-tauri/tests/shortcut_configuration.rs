use singularity_live::shortcuts::{
    AtomicConfigWriter, ShortcutAction, ShortcutBinding, ShortcutBindings, ShortcutChord,
    ShortcutConfigStore, ShortcutKey, ShortcutModifier, ShortcutPlatform, validate_bindings,
};
use std::{fs, io, path::Path, sync::Arc};

struct FailingAtomicWriter;

impl AtomicConfigWriter for FailingAtomicWriter {
    fn replace_atomically(&self, _path: &Path, _contents: &[u8]) -> io::Result<()> {
        Err(io::Error::other("simulated atomic replacement failure"))
    }
}

#[test]
fn defaults_to_capture_send_visibility_voice_and_quick_send_bindings() {
    let windows = ShortcutBindings::defaults(ShortcutPlatform::Windows);
    let macos = ShortcutBindings::defaults(ShortcutPlatform::MacOS);

    assert_eq!(windows.len(), 6);
    assert_eq!(windows[0].action, ShortcutAction::Screenshot);
    assert_eq!(
        windows[0].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+super+KeyP".to_owned())
    );
    assert_eq!(
        macos[0].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+super+KeyP".to_owned())
    );
    assert_eq!(windows[1].action, ShortcutAction::ScreenshotSend);
    assert_eq!(
        windows[1].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+alt+KeyS".to_owned())
    );
    assert_eq!(windows[2].action, ShortcutAction::ToggleTaskbarIcon);
    assert_eq!(
        windows[2].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+alt+KeyH".to_owned())
    );
    assert_eq!(windows[3].action, ShortcutAction::VoiceInput);
    assert!(windows[3].chord.is_none());
    assert_eq!(windows[4].action, ShortcutAction::QuickSend);
    assert!(windows[4].chord.is_none());
    assert_eq!(windows[5].action, ShortcutAction::MinMode);
    assert!(windows[5].chord.is_none());
}

#[test]
fn migrates_older_shortcut_files_and_persists_only_new_actions() {
    let directory = tempfile::tempdir().expect("temporary shortcut directory");
    let path = directory.path().join("shortcut-bindings.json");
    let mut old_bindings = ShortcutBindings::defaults(ShortcutPlatform::Windows);
    old_bindings.retain(|binding| {
        matches!(
            binding.action,
            ShortcutAction::Screenshot | ShortcutAction::VoiceInput | ShortcutAction::QuickSend
        )
    });
    let previous_capture_chord = old_bindings[0].chord.clone();
    let previous_file = serde_json::json!({
        "version": 1,
        "bindings": old_bindings,
    });
    fs::write(
        &path,
        serde_json::to_vec(&previous_file).expect("old shortcut config serializes"),
    )
    .expect("old shortcut config is written");
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    let loaded = store.load().expect("older config is migrated");

    assert_eq!(loaded.len(), 6);
    assert_eq!(loaded[0].action, ShortcutAction::Screenshot);
    assert_eq!(loaded[0].chord, previous_capture_chord);
    assert!(
        loaded
            .iter()
            .any(|binding| binding.action == ShortcutAction::ScreenshotSend)
    );
    assert!(
        loaded
            .iter()
            .any(|binding| binding.action == ShortcutAction::ToggleTaskbarIcon)
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("migrated config is persisted"))
            .expect("migrated config is valid JSON");
    assert_eq!(persisted["version"], 4);
    assert_eq!(
        store.load().expect("version 4 config remains stable"),
        loaded
    );
}

#[test]
fn does_not_restore_rows_removed_from_current_shortcut_configuration() {
    let directory = tempfile::tempdir().expect("temporary shortcut directory");
    let path = directory.path().join("shortcut-bindings.json");
    let mut bindings = ShortcutBindings::defaults(ShortcutPlatform::Windows);
    bindings.retain(|binding| binding.action != ShortcutAction::VoiceInput);
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    store.save(&bindings).expect("save current settings");

    let loaded = store.load().expect("load current settings");

    assert_eq!(loaded, bindings);
    assert!(
        loaded
            .iter()
            .all(|binding| binding.action != ShortcutAction::VoiceInput)
    );
}

#[test]
fn migrates_the_previous_window_toggle_action_to_the_taskbar_icon_action() {
    let directory = tempfile::tempdir().expect("temporary shortcut directory");
    let path = directory.path().join("shortcut-bindings.json");
    let mut bindings = serde_json::to_value(ShortcutBindings::defaults(ShortcutPlatform::Windows))
        .expect("bindings serialize");
    let bindings_array = bindings.as_array_mut().expect("bindings are an array");
    let previous_toggle = bindings_array
        .iter_mut()
        .find(|binding| binding["action"] == "toggle_taskbar_icon")
        .expect("taskbar toggle exists");
    previous_toggle["action"] = serde_json::Value::String("toggle_window".to_owned());
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "version": 2,
            "bindings": bindings,
        }))
        .expect("previous config serializes"),
    )
    .expect("previous config is written");
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    let loaded = store.load().expect("previous config is migrated");

    assert!(
        loaded
            .iter()
            .any(|binding| binding.action == ShortcutAction::ToggleTaskbarIcon)
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read migrated config"))
            .expect("migrated config is valid JSON");
    assert_eq!(persisted["version"], 4);
    assert!(
        persisted["bindings"]
            .as_array()
            .is_some_and(|persisted_bindings| {
                persisted_bindings
                    .iter()
                    .any(|binding| binding["action"] == "toggle_taskbar_icon")
            })
    );
}

#[test]
fn accepts_a_cleared_binding_row_but_rejects_an_empty_configuration() {
    let cleared = ShortcutBinding::new(ShortcutAction::Screenshot, None);

    assert_eq!(validate_bindings(&[cleared]), Ok(()));
    assert!(validate_bindings(&[]).is_err());
}

#[test]
fn accepts_up_to_two_modifiers_and_one_trigger_key() {
    let chord = ShortcutChord::new(
        vec![ShortcutModifier::Control, ShortcutModifier::Super],
        ShortcutKey::parse("KeyP").expect("supported key"),
    )
    .expect("three-key chord");

    assert_eq!(chord.canonical(), "ctrl+super+KeyP");
}

#[test]
fn rejects_duplicate_modifiers_and_escape_as_a_trigger() {
    assert!(
        ShortcutChord::new(
            vec![ShortcutModifier::Control, ShortcutModifier::Control],
            ShortcutKey::parse("KeyP").expect("supported key"),
        )
        .is_err()
    );
    assert!(ShortcutKey::parse("Escape").is_err());
    assert!(
        ShortcutChord::new(
            vec![
                ShortcutModifier::Control,
                ShortcutModifier::Super,
                ShortcutModifier::Alt,
            ],
            ShortcutKey::parse("KeyP").expect("supported key"),
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_chords_but_allows_repeated_actions() {
    let chord = Some(
        ShortcutChord::new(
            vec![ShortcutModifier::Control],
            ShortcutKey::parse("KeyP").expect("supported key"),
        )
        .expect("valid chord"),
    );
    let other = Some(
        ShortcutChord::new(
            vec![ShortcutModifier::Control],
            ShortcutKey::parse("KeyS").expect("supported key"),
        )
        .expect("valid chord"),
    );
    let first = ShortcutBinding::new(ShortcutAction::Screenshot, chord.clone());
    let second = ShortcutBinding::new(ShortcutAction::Screenshot, other);

    assert_eq!(validate_bindings(&[first.clone(), second]), Ok(()));
    let duplicate = ShortcutBinding::new(ShortcutAction::Screenshot, first.chord.clone());
    assert!(matches!(
        validate_bindings(&[first, duplicate]),
        Err(singularity_live::shortcuts::BindingError::DuplicateChord)
    ));
}

#[test]
fn persists_versioned_bindings_and_uses_defaults_when_the_file_is_missing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("shortcuts.json");
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    let defaults = store.load().expect("missing file uses defaults");
    assert_eq!(defaults.len(), 6);
    store.save(&defaults).expect("save bindings");
    assert_eq!(store.load().expect("load saved bindings"), defaults);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(path).expect("read saved config")
        )
        .expect("valid JSON")["version"],
        4
    );
}

#[test]
fn rejects_malformed_and_unsupported_versioned_configuration() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("shortcuts.json");
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    std::fs::write(&path, b"not json").expect("write malformed config");
    assert!(store.load().is_err());
    std::fs::write(&path, br#"{"version": 5, "bindings": []}"#).expect("write unsupported version");
    assert!(store.load().is_err());
}

#[test]
fn reports_an_atomic_write_failure_without_mutating_existing_config() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("shortcuts.json");
    let original = br#"{"version":1,"bindings":[]}"#;
    std::fs::write(&path, original).expect("write existing config");
    let store = ShortcutConfigStore::with_writer(
        &path,
        ShortcutPlatform::Windows,
        Arc::new(FailingAtomicWriter),
    );
    let replacement = ShortcutBindings::defaults(ShortcutPlatform::Windows);

    assert!(store.save(&replacement).is_err());
    assert_eq!(
        std::fs::read(path).expect("read unchanged config"),
        original
    );
}

#[test]
fn unavailable_config_directory_fails_without_using_the_working_directory() {
    let store = ShortcutConfigStore::unavailable(ShortcutPlatform::Windows);
    let bindings = ShortcutBindings::defaults(ShortcutPlatform::Windows);

    assert!(store.load().is_err());
    assert!(store.save(&bindings).is_err());
}
