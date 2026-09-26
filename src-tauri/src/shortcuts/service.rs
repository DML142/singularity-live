use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

use super::{
    BindingError, BindingStoreError, ShortcutBinding, ShortcutBindingId, ShortcutConfigStore,
    validate_bindings,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum ShortcutRegistrationState {
    Registered { effective_trigger: String },
    Unbound,
    Failed { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindingView {
    pub binding: ShortcutBinding,
    pub registration: ShortcutRegistrationState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRegistrationFailure {
    pub binding_id: ShortcutBindingId,
    pub message: String,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ShortcutRegistrarError {
    #[error("Shortcut registration is unavailable")]
    Unavailable,
    #[error("One or more shortcuts were rejected by the operating system")]
    Rejected {
        failures: Vec<BindingRegistrationFailure>,
    },
}

/// Registers usable startup rows and replaces a running binding set transactionally.
#[async_trait]
pub trait ShortcutRegistrar: Send + Sync {
    async fn register_available(&self, bindings: &[ShortcutBinding]) -> Vec<ShortcutBindingView>;

    async fn replace(
        &self,
        bindings: &[ShortcutBinding],
    ) -> Result<Vec<ShortcutBindingView>, ShortcutRegistrarError>;

    async fn unregister_all(&self) -> Result<(), ShortcutRegistrarError>;
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ShortcutServiceError {
    #[error(transparent)]
    InvalidBindings(#[from] BindingError),
    #[error("Shortcut configuration is unavailable")]
    Configuration,
    #[error("One or more shortcuts were rejected by the operating system")]
    RegistrationRejected {
        failures: Vec<BindingRegistrationFailure>,
    },
    #[error("Shortcut registration is unavailable")]
    RegistrarUnavailable,
    #[error("Shortcut registrations could not be restored after an update failed")]
    Rollback,
    #[error("Shortcut registrar returned an inconsistent state")]
    InvalidRegistrarState,
}

struct BindingServiceState {
    initialized: bool,
    views: Vec<ShortcutBindingView>,
}

pub struct ShortcutBindingService {
    store: Arc<ShortcutConfigStore>,
    registrar: Arc<dyn ShortcutRegistrar>,
    state: Mutex<BindingServiceState>,
}

impl ShortcutBindingService {
    #[must_use]
    pub fn new(store: Arc<ShortcutConfigStore>, registrar: Arc<dyn ShortcutRegistrar>) -> Self {
        Self {
            store,
            registrar,
            state: Mutex::new(BindingServiceState {
                initialized: false,
                views: Vec::new(),
            }),
        }
    }

    /// Loads the persisted rows and registers every usable shortcut independently.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable or invalid configuration. Per-row OS conflicts are
    /// returned as failed registration states so unrelated rows can remain active.
    pub async fn initialize(&self) -> Result<Vec<ShortcutBindingView>, ShortcutServiceError> {
        let mut state = self.state.lock().await;
        if state.initialized {
            return Ok(state.views.clone());
        }
        let bindings = self.store.load().map_err(map_store_error)?;
        let views = self.registration_views(&bindings).await;
        state.views.clone_from(&views);
        state.initialized = true;
        Ok(views)
    }

    /// Applies a candidate set, persisting only after all configured chords register.
    ///
    /// # Errors
    ///
    /// Returns validation, registration, config-write, or rollback failure. Failed updates
    /// retain the prior config and attempt to restore its OS registrations.
    pub async fn update_bindings(
        &self,
        candidate: Vec<ShortcutBinding>,
    ) -> Result<Vec<ShortcutBindingView>, ShortcutServiceError> {
        validate_bindings(&candidate)?;
        if !self.state.lock().await.initialized {
            self.initialize().await?;
        }
        let mut state = self.state.lock().await;
        let previous = state
            .views
            .iter()
            .map(|view| view.binding.clone())
            .collect::<Vec<_>>();
        let registered_views = match self.registrar.replace(&candidate).await {
            Ok(views) => views,
            Err(ShortcutRegistrarError::Rejected { failures }) => {
                return Err(ShortcutServiceError::RegistrationRejected { failures });
            }
            Err(ShortcutRegistrarError::Unavailable) => {
                return Err(ShortcutServiceError::RegistrarUnavailable);
            }
        };
        let Some(candidate_views) = normalize_views(&candidate, registered_views) else {
            self.restore(&previous, &mut state).await?;
            return Err(ShortcutServiceError::InvalidRegistrarState);
        };
        let failures = registration_failures(&candidate_views);
        if !failures.is_empty() {
            self.restore(&previous, &mut state).await?;
            return Err(ShortcutServiceError::RegistrationRejected { failures });
        }
        if self.store.save(&candidate).is_err() {
            self.restore(&previous, &mut state).await?;
            return Err(ShortcutServiceError::Configuration);
        }
        state.views.clone_from(&candidate_views);
        Ok(candidate_views)
    }

    /// Returns the latest persisted rows with their current native registration state.
    pub async fn snapshot(&self) -> Vec<ShortcutBindingView> {
        self.state.lock().await.views.clone()
    }

    /// Unregisters app-owned shortcuts during orderly application shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS adapter cannot release its registrations.
    pub async fn unregister_all(&self) -> Result<(), ShortcutServiceError> {
        self.registrar
            .unregister_all()
            .await
            .map_err(|_| ShortcutServiceError::Rollback)
    }

    async fn registration_views(&self, bindings: &[ShortcutBinding]) -> Vec<ShortcutBindingView> {
        normalize_views(bindings, self.registrar.register_available(bindings).await).unwrap_or_else(
            || failed_views(bindings, "Shortcut registration returned an invalid state"),
        )
    }

    async fn restore(
        &self,
        previous: &[ShortcutBinding],
        state: &mut BindingServiceState,
    ) -> Result<(), ShortcutServiceError> {
        let views = self
            .registrar
            .replace(previous)
            .await
            .map_err(|_| ShortcutServiceError::Rollback)?;
        state.views = normalize_views(previous, views).ok_or(ShortcutServiceError::Rollback)?;
        Ok(())
    }
}

fn normalize_views(
    bindings: &[ShortcutBinding],
    views: Vec<ShortcutBindingView>,
) -> Option<Vec<ShortcutBindingView>> {
    let mut by_id = HashMap::new();
    for view in views {
        if by_id.insert(view.binding.id, view).is_some() {
            return None;
        }
    }
    if by_id.len() != bindings.len() {
        return None;
    }
    bindings
        .iter()
        .map(|binding| {
            let mut view = by_id.remove(&binding.id)?;
            if view.binding != *binding {
                return None;
            }
            if binding.chord.is_none()
                && !matches!(view.registration, ShortcutRegistrationState::Unbound)
            {
                return None;
            }
            if binding.chord.is_some()
                && matches!(view.registration, ShortcutRegistrationState::Unbound)
            {
                return None;
            }
            view.binding = binding.clone();
            Some(view)
        })
        .collect()
}

fn registration_failures(views: &[ShortcutBindingView]) -> Vec<BindingRegistrationFailure> {
    views
        .iter()
        .filter_map(|view| match &view.registration {
            ShortcutRegistrationState::Failed { message } => Some(BindingRegistrationFailure {
                binding_id: view.binding.id,
                message: message.clone(),
            }),
            ShortcutRegistrationState::Registered { .. } | ShortcutRegistrationState::Unbound => {
                None
            }
        })
        .collect()
}

fn failed_views(bindings: &[ShortcutBinding], message: &str) -> Vec<ShortcutBindingView> {
    bindings
        .iter()
        .map(|binding| ShortcutBindingView {
            binding: binding.clone(),
            registration: if binding.chord.is_some() {
                ShortcutRegistrationState::Failed {
                    message: message.to_owned(),
                }
            } else {
                ShortcutRegistrationState::Unbound
            },
        })
        .collect()
}

fn map_store_error(_error: BindingStoreError) -> ShortcutServiceError {
    ShortcutServiceError::Configuration
}
