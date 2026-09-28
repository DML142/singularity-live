use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutPlatform {
    Windows,
    Linux,
    MacOS,
}

impl ShortcutPlatform {
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOS
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    Screenshot,
    ScreenshotSend,
    #[serde(alias = "toggle_window")]
    ToggleTaskbarIcon,
    VoiceInput,
    QuickSend,
    MinMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ShortcutBindingId(Uuid);

impl ShortcutBindingId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ShortcutBindingId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ShortcutBindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutBinding {
    pub id: ShortcutBindingId,
    pub action: ShortcutAction,
    pub chord: Option<ShortcutChord>,
}

impl ShortcutBinding {
    #[must_use]
    pub fn new(action: ShortcutAction, chord: Option<ShortcutChord>) -> Self {
        Self {
            id: ShortcutBindingId::new(),
            action,
            chord,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutModifier {
    Control,
    Alt,
    Shift,
    Super,
}

impl ShortcutModifier {
    const fn canonical(self) -> &'static str {
        match self {
            Self::Control => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Super => "super",
        }
    }

    const fn sort_order(self) -> u8 {
        match self {
            Self::Control => 0,
            Self::Alt => 1,
            Self::Shift => 2,
            Self::Super => 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ShortcutKey(String);

impl ShortcutKey {
    /// Parses a physical keyboard code supported by all current desktop adapters.
    ///
    /// # Errors
    ///
    /// Returns a validation error for unsupported keys and Escape.
    pub fn parse(value: &str) -> Result<Self, BindingError> {
        if value == "Escape" {
            return Err(BindingError::EscapeCannotBeAssigned);
        }
        if !is_supported_key(value) {
            return Err(BindingError::UnsupportedKey);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<(), BindingError> {
        if self.0 == "Escape" {
            Err(BindingError::EscapeCannotBeAssigned)
        } else if is_supported_key(&self.0) {
            Ok(())
        } else {
            Err(BindingError::UnsupportedKey)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutChord {
    pub modifiers: Vec<ShortcutModifier>,
    pub key: ShortcutKey,
}

impl ShortcutChord {
    /// Creates a simultaneous chord with at most two modifiers and one trigger key.
    ///
    /// # Errors
    ///
    /// Returns a validation error for repeated/excess modifiers or an unsupported key.
    pub fn new(
        mut modifiers: Vec<ShortcutModifier>,
        key: ShortcutKey,
    ) -> Result<Self, BindingError> {
        key.validate()?;
        if modifiers.len() > 2 {
            return Err(BindingError::TooManyKeys);
        }
        let original_len = modifiers.len();
        modifiers.sort_by_key(|modifier| modifier.sort_order());
        modifiers.dedup();
        if modifiers.len() != original_len {
            return Err(BindingError::RepeatedModifier);
        }
        Ok(Self { modifiers, key })
    }

    #[must_use]
    pub fn canonical(&self) -> String {
        let mut modifiers = self.modifiers.clone();
        modifiers.sort_by_key(|modifier| modifier.sort_order());
        modifiers
            .iter()
            .map(|modifier| modifier.canonical())
            .chain(std::iter::once(self.key.code()))
            .collect::<Vec<_>>()
            .join("+")
    }

    fn validate(&self) -> Result<(), BindingError> {
        if self.modifiers.len() > 2 {
            return Err(BindingError::TooManyKeys);
        }
        let unique = self.modifiers.iter().copied().collect::<HashSet<_>>();
        if unique.len() != self.modifiers.len() {
            return Err(BindingError::RepeatedModifier);
        }
        self.key.validate()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum BindingError {
    #[error("At least one binding row is required")]
    EmptyConfiguration,
    #[error("Binding identifiers must be unique")]
    DuplicateBindingId,
    #[error("Shortcut chords must be unique")]
    DuplicateChord,
    #[error("A shortcut can contain at most three simultaneous keys")]
    TooManyKeys,
    #[error("A modifier key cannot be repeated")]
    RepeatedModifier,
    #[error("Escape clears a binding and cannot be assigned")]
    EscapeCannotBeAssigned,
    #[error("The selected key is not supported")]
    UnsupportedKey,
}

/// Provides platform-independent bindings and validates all persisted or IPC values.
pub struct ShortcutBindings;

impl ShortcutBindings {
    #[must_use]
    pub fn defaults(_platform: ShortcutPlatform) -> Vec<ShortcutBinding> {
        vec![
            ShortcutBinding::new(
                ShortcutAction::Screenshot,
                Some(ShortcutChord {
                    modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Super],
                    key: ShortcutKey("KeyP".to_owned()),
                }),
            ),
            ShortcutBinding::new(
                ShortcutAction::ScreenshotSend,
                Some(ShortcutChord {
                    modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Alt],
                    key: ShortcutKey("KeyS".to_owned()),
                }),
            ),
            ShortcutBinding::new(
                ShortcutAction::ToggleTaskbarIcon,
                Some(ShortcutChord {
                    modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Alt],
                    key: ShortcutKey("KeyH".to_owned()),
                }),
            ),
            ShortcutBinding::new(ShortcutAction::VoiceInput, None),
            ShortcutBinding::new(ShortcutAction::QuickSend, None),
            ShortcutBinding::new(ShortcutAction::MinMode, None),
        ]
    }
}

/// Validates binding rows independently from their UI or OS registration state.
///
/// # Errors
///
/// Returns the first invalid or duplicate row condition.
pub fn validate_bindings(bindings: &[ShortcutBinding]) -> Result<(), BindingError> {
    if bindings.is_empty() {
        return Err(BindingError::EmptyConfiguration);
    }
    let mut ids = HashSet::new();
    let mut chords = HashSet::new();
    for binding in bindings {
        if !ids.insert(binding.id) {
            return Err(BindingError::DuplicateBindingId);
        }
        if let Some(chord) = &binding.chord {
            chord.validate()?;
            if !chords.insert(chord.canonical()) {
                return Err(BindingError::DuplicateChord);
            }
        }
    }
    Ok(())
}

fn is_supported_key(value: &str) -> bool {
    let Some(letter) = value.strip_prefix("Key") else {
        return if let Some(digit) = value.strip_prefix("Digit") {
            digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit()
        } else if let Some(function_key) = value.strip_prefix('F') {
            function_key
                .parse::<u8>()
                .is_ok_and(|number| (1..=24).contains(&number))
        } else {
            matches!(
                value,
                "Backquote"
                    | "Backslash"
                    | "Backspace"
                    | "BracketLeft"
                    | "BracketRight"
                    | "Comma"
                    | "Delete"
                    | "End"
                    | "Enter"
                    | "Equal"
                    | "Home"
                    | "Insert"
                    | "Minus"
                    | "PageDown"
                    | "PageUp"
                    | "Period"
                    | "Quote"
                    | "Semicolon"
                    | "Slash"
                    | "Space"
                    | "Tab"
                    | "ArrowDown"
                    | "ArrowLeft"
                    | "ArrowRight"
                    | "ArrowUp"
            )
        };
    };
    letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase()
}
