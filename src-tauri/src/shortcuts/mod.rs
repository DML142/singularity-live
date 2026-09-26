pub(crate) mod capture_coordinator;
mod model;
mod registrar;
mod service;
mod store;

pub use model::{
    BindingError, ShortcutAction, ShortcutBinding, ShortcutBindingId, ShortcutBindings,
    ShortcutChord, ShortcutKey, ShortcutModifier, ShortcutPlatform, validate_bindings,
};
pub(crate) use registrar::platform_shortcut_registrar;
pub use service::{
    BindingRegistrationFailure, ShortcutBindingService, ShortcutBindingView, ShortcutRegistrar,
    ShortcutRegistrarError, ShortcutRegistrationState, ShortcutServiceError,
};
pub use store::{AtomicConfigWriter, BindingStoreError, ShortcutConfigStore};
