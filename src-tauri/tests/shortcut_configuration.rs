use singularity_live::shortcuts::{
    AtomicConfigWriter, ShortcutAction, ShortcutBinding, ShortcutBindings, ShortcutChord,
    ShortcutConfigStore, ShortcutKey, ShortcutModifier, ShortcutPlatform, validate_bindings,
};
use std::{io, path::Path, sync::Arc};

struct FailingAtomicWriter;

impl AtomicConfigWriter for FailingAtomicWriter {
    fn replace_atomically(&self, _path: &Path, _contents: &[u8]) -> io::Result<()> {
        Err(io::Error::other("simulated atomic replacement failure"))
    }
}

#[test]
fn defaults_to_one_screenshot_binding_with_platform_super_key() {
    let windows = ShortcutBindings::defaults(ShortcutPlatform::Windows);
    let macos = ShortcutBindings::defaults(ShortcutPlatform::MacOS);

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].action, ShortcutAction::Screenshot);
    assert_eq!(
        windows[0].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+super+KeyP".to_owned())
    );
    assert_eq!(
        macos[0].chord.as_ref().map(ShortcutChord::canonical),
        Some("ctrl+super+KeyP".to_owned())
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
    assert_eq!(defaults.len(), 1);
    store.save(&defaults).expect("save bindings");
    assert_eq!(store.load().expect("load saved bindings"), defaults);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(path).expect("read saved config")
        )
        .expect("valid JSON")["version"],
        1
    );
}

#[test]
fn rejects_malformed_and_unsupported_versioned_configuration() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("shortcuts.json");
    let store = ShortcutConfigStore::new(&path, ShortcutPlatform::Windows);

    std::fs::write(&path, b"not json").expect("write malformed config");
    assert!(store.load().is_err());
    std::fs::write(&path, br#"{"version": 2, "bindings": []}"#).expect("write unsupported version");
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
