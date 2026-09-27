use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use async_trait::async_trait;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use super::{
    BindingRegistrationFailure, ShortcutBinding, ShortcutBindingId, ShortcutBindingView,
    ShortcutChord, ShortcutRegistrar, ShortcutRegistrarError, ShortcutRegistrationState,
};

pub(crate) type ShortcutActivationHandler = Arc<dyn Fn() + Send + Sync + 'static>;

pub(crate) fn platform_shortcut_registrar(
    app: AppHandle,
    activation: ShortcutActivationHandler,
) -> Arc<dyn ShortcutRegistrar> {
    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        return Arc::new(wayland::PortalShortcutRegistrar::new(activation));
    }

    Arc::new(TauriShortcutRegistrar::new(app, activation))
}

pub fn native_shortcut_expression(chord: &ShortcutChord) -> String {
    chord.canonical()
}

#[cfg(any(target_os = "linux", test))]
pub fn portal_preferred_trigger(chord: &ShortcutChord) -> Option<String> {
    let modifiers = chord
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            super::ShortcutModifier::Control => "CTRL",
            super::ShortcutModifier::Alt => "ALT",
            super::ShortcutModifier::Shift => "SHIFT",
            super::ShortcutModifier::Super => "LOGO",
        })
        .collect::<Vec<_>>();
    let key = portal_key_name(chord.key.code())?;
    Some(
        modifiers
            .into_iter()
            .chain(std::iter::once(key))
            .collect::<Vec<_>>()
            .join("+"),
    )
}

#[cfg(any(target_os = "linux", test))]
fn portal_key_name(code: &str) -> Option<&str> {
    if let Some(letter) = code.strip_prefix("Key") {
        return (letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase()).then_some(letter);
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return (digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit()).then_some(digit);
    }
    Some(match code {
        "Backquote" => "grave",
        "Backslash" => "backslash",
        "Backspace" => "BackSpace",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        "Comma" => "comma",
        "Delete" => "Delete",
        "End" => "End",
        "Equal" => "equal",
        "Enter" => "Return",
        "Home" => "Home",
        "Insert" => "Insert",
        "Minus" => "minus",
        "PageDown" => "Page_Down",
        "PageUp" => "Page_Up",
        "Period" => "period",
        "Quote" => "apostrophe",
        "Semicolon" => "semicolon",
        "Slash" => "slash",
        "Space" => "space",
        "Tab" => "Tab",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "ArrowUp" => "Up",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "F13" => "F13",
        "F14" => "F14",
        "F15" => "F15",
        "F16" => "F16",
        "F17" => "F17",
        "F18" => "F18",
        "F19" => "F19",
        "F20" => "F20",
        "F21" => "F21",
        "F22" => "F22",
        "F23" => "F23",
        "F24" => "F24",
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

struct TauriShortcutRegistrar {
    app: AppHandle,
    activation: ShortcutActivationHandler,
    active: Mutex<HashSet<String>>,
}

impl TauriShortcutRegistrar {
    fn new(app: AppHandle, activation: ShortcutActivationHandler) -> Self {
        Self {
            app,
            activation,
            active: Mutex::new(HashSet::new()),
        }
    }

    fn active_lock(&self) -> MutexGuard<'_, HashSet<String>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn register_expression(&self, expression: &str) -> Result<(), String> {
        let activation = Arc::clone(&self.activation);
        self.app
            .global_shortcut()
            .on_shortcut(expression, move |_app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    activation();
                }
            })
            .map_err(|error| format!("Shortcut registration failed: {error}"))
    }

    fn unregister_expression(&self, expression: &str) -> Result<(), String> {
        self.app
            .global_shortcut()
            .unregister(expression)
            .map_err(|_| "This shortcut could not be released".to_owned())
    }

    fn outcome_views(
        bindings: &[ShortcutBinding],
        failed: &HashMap<ShortcutBindingId, String>,
    ) -> Vec<ShortcutBindingView> {
        bindings
            .iter()
            .map(|binding| ShortcutBindingView {
                binding: binding.clone(),
                registration: match &binding.chord {
                    None => ShortcutRegistrationState::Unbound,
                    Some(_chord) if failed.contains_key(&binding.id) => {
                        ShortcutRegistrationState::Failed {
                            message: failed
                                .get(&binding.id)
                                .cloned()
                                .unwrap_or_else(|| "Shortcut registration failed".to_owned()),
                        }
                    }
                    Some(chord) => ShortcutRegistrationState::Registered {
                        effective_trigger: native_shortcut_expression(chord),
                    },
                },
            })
            .collect()
    }

    fn register_candidates(
        &self,
        bindings: &[ShortcutBinding],
        active: &HashSet<String>,
    ) -> (HashSet<String>, HashMap<ShortcutBindingId, String>) {
        let mut added = HashSet::new();
        let mut failed = HashMap::new();
        for binding in bindings {
            let Some(chord) = &binding.chord else {
                continue;
            };
            let expression = native_shortcut_expression(chord);
            if active.contains(&expression) || added.contains(&expression) {
                continue;
            }
            match self.register_expression(&expression) {
                Ok(()) => {
                    added.insert(expression);
                }
                Err(message) => {
                    failed.insert(binding.id, message);
                }
            }
        }
        (added, failed)
    }
}

#[async_trait]
impl ShortcutRegistrar for TauriShortcutRegistrar {
    async fn register_available(&self, bindings: &[ShortcutBinding]) -> Vec<ShortcutBindingView> {
        let active = self.active_lock().clone();
        let (added, failed) = self.register_candidates(bindings, &active);
        self.active_lock().extend(added);
        Self::outcome_views(bindings, &failed)
    }

    async fn replace(
        &self,
        bindings: &[ShortcutBinding],
    ) -> Result<Vec<ShortcutBindingView>, ShortcutRegistrarError> {
        let previous = self.active_lock().clone();
        let (added, failures_by_id) = self.register_candidates(bindings, &previous);
        if !failures_by_id.is_empty() {
            for expression in &added {
                let _ = self.unregister_expression(expression);
            }
            let failures = bindings
                .iter()
                .filter(|binding| failures_by_id.contains_key(&binding.id))
                .map(|binding| BindingRegistrationFailure {
                    binding_id: binding.id,
                    message: failures_by_id
                        .get(&binding.id)
                        .cloned()
                        .unwrap_or_else(|| "Shortcut registration failed".to_owned()),
                })
                .collect();
            return Err(ShortcutRegistrarError::Rejected { failures });
        }
        let desired = bindings
            .iter()
            .filter_map(|binding| binding.chord.as_ref())
            .map(native_shortcut_expression)
            .collect::<HashSet<_>>();
        let removed = previous.difference(&desired).cloned().collect::<Vec<_>>();
        let mut released: Vec<String> = Vec::new();
        for expression in &removed {
            if self.unregister_expression(expression).is_err() {
                for prior in &released {
                    let _ = self.register_expression(prior);
                }
                for added_expression in &added {
                    let _ = self.unregister_expression(added_expression);
                }
                return Err(ShortcutRegistrarError::Unavailable);
            }
            released.push(expression.clone());
        }
        let mut current = self.active_lock();
        *current = desired;
        Ok(Self::outcome_views(bindings, &HashMap::new()))
    }

    async fn unregister_all(&self) -> Result<(), ShortcutRegistrarError> {
        let active = self.active_lock().clone();
        let mut failed = HashSet::new();
        for expression in &active {
            if self.unregister_expression(expression).is_err() {
                failed.insert(expression.clone());
            }
        }
        *self.active_lock() = failed;
        if self.active_lock().is_empty() {
            Ok(())
        } else {
            Err(ShortcutRegistrarError::Unavailable)
        }
    }
}

#[cfg(target_os = "linux")]
mod wayland;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{native_shortcut_expression, portal_preferred_trigger};
    use crate::shortcuts::{ShortcutChord, ShortcutKey, ShortcutModifier};
    use tauri_plugin_global_shortcut::Shortcut;

    fn default_chord() -> ShortcutChord {
        ShortcutChord::new(
            vec![ShortcutModifier::Control, ShortcutModifier::Super],
            ShortcutKey::parse("KeyP").expect("supported key"),
        )
        .expect("valid chord")
    }

    #[test]
    fn native_shortcut_uses_physical_key_codes_and_platform_super_modifier() {
        let expression = native_shortcut_expression(&default_chord());
        assert_eq!(expression, "ctrl+super+KeyP");
        assert!(Shortcut::from_str(&expression).is_ok());
    }

    #[test]
    fn wayland_shortcut_uses_xdg_keysym_trigger_not_a_sequence() {
        assert_eq!(
            portal_preferred_trigger(&default_chord()).as_deref(),
            Some("CTRL+LOGO+P")
        );
    }

    #[test]
    fn wayland_shortcut_maps_supported_punctuation_codes_to_xkb_keysyms() {
        let mappings = [
            ("Backquote", "grave"),
            ("Backslash", "backslash"),
            ("BracketLeft", "bracketleft"),
            ("BracketRight", "bracketright"),
            ("Comma", "comma"),
            ("Equal", "equal"),
            ("Minus", "minus"),
            ("Period", "period"),
            ("Quote", "apostrophe"),
            ("Semicolon", "semicolon"),
            ("Slash", "slash"),
        ];

        for (code, keysym) in mappings {
            let chord = ShortcutChord::new(
                vec![ShortcutModifier::Control],
                ShortcutKey::parse(code).expect("supported punctuation key"),
            )
            .expect("valid chord");
            assert_eq!(
                portal_preferred_trigger(&chord).as_deref(),
                Some(format!("CTRL+{keysym}").as_str())
            );
        }
    }
}
