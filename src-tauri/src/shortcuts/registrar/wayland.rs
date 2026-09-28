use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ashpd::desktop::{
    CreateSessionOptions,
    global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
};
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::super::{
    BindingRegistrationFailure, ShortcutAction, ShortcutBinding, ShortcutBindingView,
    ShortcutRegistrar, ShortcutRegistrarError, ShortcutRegistrationState,
};
use super::{ShortcutActivationHandler, portal_preferred_trigger};

#[derive(Clone, Debug, Eq, PartialEq)]
struct PortalShortcutRequest {
    binding_id: String,
    preferred_trigger: String,
    action: ShortcutAction,
}

#[async_trait]
trait PortalSessionHandle: Send + Sync {
    async fn close(&self) -> Result<(), PortalCloseError>;
}

struct PortalCloseError;

struct PortalBoundShortcuts {
    effective_triggers: HashMap<String, String>,
    handle: Arc<dyn PortalSessionHandle>,
}

impl PortalBoundShortcuts {
    fn new(
        effective_triggers: HashMap<String, String>,
        handle: Arc<dyn PortalSessionHandle>,
    ) -> Self {
        Self {
            effective_triggers,
            handle,
        }
    }

    async fn close(&self) -> Result<(), PortalCloseError> {
        self.handle.close().await
    }
}

#[async_trait]
trait GlobalShortcutsPortal: Send + Sync {
    async fn bind(
        &self,
        requests: &[PortalShortcutRequest],
        activation: ShortcutActivationHandler,
    ) -> Result<PortalBoundShortcuts, ShortcutRegistrarError>;
}

struct AshpdGlobalShortcutsPortal;

struct AshpdPortalSessionHandle {
    session: ashpd::desktop::Session<GlobalShortcuts>,
    cancellation: CancellationToken,
}

#[async_trait]
impl PortalSessionHandle for AshpdPortalSessionHandle {
    async fn close(&self) -> Result<(), PortalCloseError> {
        self.session.close().await.map_err(|_| PortalCloseError)?;
        self.cancellation.cancel();
        Ok(())
    }
}

pub(super) struct PortalShortcutRegistrar {
    state: Mutex<Option<PortalSessionState>>,
    portal: Arc<dyn GlobalShortcutsPortal>,
    activation: ShortcutActivationHandler,
}

struct PortalSessionState {
    portal_session: PortalBoundShortcuts,
    bindings: Vec<ShortcutBinding>,
    views: Vec<ShortcutBindingView>,
}

impl PortalShortcutRegistrar {
    pub(super) fn new(activation: ShortcutActivationHandler) -> Self {
        Self::with_portal(Arc::new(AshpdGlobalShortcutsPortal), activation)
    }

    fn with_portal(
        portal: Arc<dyn GlobalShortcutsPortal>,
        activation: ShortcutActivationHandler,
    ) -> Self {
        Self {
            state: Mutex::new(None),
            portal,
            activation,
        }
    }

    async fn install(
        &self,
        bindings: &[ShortcutBinding],
    ) -> Result<(Option<PortalSessionState>, Vec<ShortcutBindingView>), ShortcutRegistrarError>
    {
        let mut unsupported = HashSet::new();
        let mut requests = Vec::new();
        for binding in bindings {
            let Some(chord) = &binding.chord else {
                continue;
            };
            let Some(trigger) = portal_preferred_trigger(chord) else {
                unsupported.insert(binding.id);
                continue;
            };
            let id = binding.id.to_string();
            requests.push(PortalShortcutRequest {
                binding_id: id,
                preferred_trigger: trigger,
                action: binding.action,
            });
        }
        if requests.is_empty() {
            let views = bindings
                .iter()
                .map(|binding| ShortcutBindingView {
                    binding: binding.clone(),
                    registration: if unsupported.contains(&binding.id) {
                        ShortcutRegistrationState::Failed {
                            message: "This key is unavailable through the system shortcut portal"
                                .to_owned(),
                        }
                    } else {
                        ShortcutRegistrationState::Unbound
                    },
                })
                .collect();
            return Ok((None, views));
        }

        let portal_session = self
            .portal
            .bind(&requests, Arc::clone(&self.activation))
            .await?;
        let effective_triggers = &portal_session.effective_triggers;
        let views = bindings
            .iter()
            .map(|binding| {
                let registration = if binding.chord.is_none() {
                    ShortcutRegistrationState::Unbound
                } else if unsupported.contains(&binding.id) {
                    ShortcutRegistrationState::Failed {
                        message: "This key is unavailable through the system shortcut portal"
                            .to_owned(),
                    }
                } else if let Some(trigger) = effective_triggers.get(&binding.id.to_string()) {
                    ShortcutRegistrationState::Registered {
                        effective_trigger: trigger.clone(),
                    }
                } else {
                    ShortcutRegistrationState::Failed {
                        message: "The system did not register this shortcut".to_owned(),
                    }
                };
                ShortcutBindingView {
                    binding: binding.clone(),
                    registration,
                }
            })
            .collect::<Vec<_>>();
        Ok((
            Some(PortalSessionState {
                portal_session,
                bindings: bindings.to_vec(),
                views: views.clone(),
            }),
            views,
        ))
    }

    async fn close_current(
        &self,
        state: &mut Option<PortalSessionState>,
    ) -> Result<Vec<ShortcutBinding>, ShortcutRegistrarError> {
        let Some(current) = state.as_ref() else {
            return Ok(Vec::new());
        };
        current
            .portal_session
            .close()
            .await
            .map_err(|_| ShortcutRegistrarError::Unavailable)?;
        let Some(current) = state.take() else {
            return Ok(Vec::new());
        };
        Ok(current.bindings)
    }

    async fn restore(
        &self,
        state: &mut Option<PortalSessionState>,
        bindings: &[ShortcutBinding],
    ) -> Result<(), ShortcutRegistrarError> {
        if bindings.is_empty() {
            *state = None;
            return Ok(());
        }
        let (restored, _) = self.install(bindings).await?;
        *state = restored;
        Ok(())
    }
}

#[async_trait]
impl ShortcutRegistrar for PortalShortcutRegistrar {
    async fn register_available(&self, bindings: &[ShortcutBinding]) -> Vec<ShortcutBindingView> {
        let mut state = self.state.lock().await;
        if let Some(current) = state.as_ref()
            && current.bindings == bindings
        {
            return current.views.clone();
        }
        match self.install(bindings).await {
            Ok((installed, views)) => {
                *state = installed;
                views
            }
            Err(_) => failed_views(bindings, "The system shortcut portal is unavailable"),
        }
    }

    async fn replace(
        &self,
        bindings: &[ShortcutBinding],
    ) -> Result<Vec<ShortcutBindingView>, ShortcutRegistrarError> {
        let mut state = self.state.lock().await;
        let previous = self.close_current(&mut state).await?;
        let result = self.install(bindings).await;
        match result {
            Ok((installed, views)) => {
                let failures = registration_failures(&views);
                if failures.is_empty() {
                    *state = installed;
                    Ok(views)
                } else {
                    if let Some(rejected) = installed {
                        let _ = rejected.portal_session.close().await;
                    }
                    self.restore(&mut state, &previous).await?;
                    Err(ShortcutRegistrarError::Rejected { failures })
                }
            }
            Err(error) => {
                self.restore(&mut state, &previous).await?;
                Err(error)
            }
        }
    }

    async fn unregister_all(&self) -> Result<(), ShortcutRegistrarError> {
        let mut state = self.state.lock().await;
        let _ = self.close_current(&mut state).await?;
        Ok(())
    }
}

#[async_trait]
impl GlobalShortcutsPortal for AshpdGlobalShortcutsPortal {
    async fn bind(
        &self,
        requests: &[PortalShortcutRequest],
        activation: ShortcutActivationHandler,
    ) -> Result<PortalBoundShortcuts, ShortcutRegistrarError> {
        let portal = GlobalShortcuts::new()
            .await
            .map_err(|_| ShortcutRegistrarError::Unavailable)?;
        let mut activated = portal
            .receive_activated()
            .await
            .map_err(|_| ShortcutRegistrarError::Unavailable)?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|_| ShortcutRegistrarError::Unavailable)?;
        let shortcuts = requests
            .iter()
            .map(|request| {
                let description = match request.action {
                    ShortcutAction::Screenshot => "Capture the current screen",
                    ShortcutAction::VoiceInput => "Toggle voice input",
                };
                NewShortcut::new(&request.binding_id, description)
                    .preferred_trigger(Some(request.preferred_trigger.as_str()))
            })
            .collect::<Vec<_>>();
        let Ok(binding_request) = portal
            .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
            .await
        else {
            let _ = session.close().await;
            return Err(ShortcutRegistrarError::Unavailable);
        };
        let Ok(response) = binding_request.response() else {
            let _ = session.close().await;
            return Err(ShortcutRegistrarError::Unavailable);
        };
        let effective_triggers = response
            .shortcuts()
            .iter()
            .map(|shortcut| {
                (
                    shortcut.id().to_owned(),
                    shortcut.trigger_description().to_owned(),
                )
            })
            .collect::<HashMap<_, _>>();
        let active_ids = effective_triggers.keys().cloned().collect::<HashSet<_>>();
        let action_by_id = requests
            .iter()
            .map(|request| (request.binding_id.clone(), request.action))
            .collect::<HashMap<_, _>>();
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    event = activated.next() => {
                        let Some(event) = event else {
                            break;
                        };
                        if active_ids.contains(event.shortcut_id()) {
                            if let Some(action) = action_by_id.get(event.shortcut_id()).copied() {
                                activation(action);
                            }
                        }
                    }
                }
            }
        });
        Ok(PortalBoundShortcuts::new(
            effective_triggers,
            Arc::new(AshpdPortalSessionHandle {
                session,
                cancellation,
            }),
        ))
    }
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

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;

    use super::{
        GlobalShortcutsPortal, PortalBoundShortcuts, PortalSessionHandle, PortalShortcutRegistrar,
        PortalShortcutRequest,
    };
    use crate::shortcuts::{
        ShortcutAction, ShortcutBinding, ShortcutChord, ShortcutKey, ShortcutModifier,
        ShortcutRegistrar, ShortcutRegistrationState,
    };

    #[derive(Default)]
    struct FakePortal {
        requests: Mutex<Vec<Vec<PortalShortcutRequest>>>,
        responses: Mutex<VecDeque<HashMap<String, String>>>,
        closed_sessions: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl GlobalShortcutsPortal for FakePortal {
        async fn bind(
            &self,
            requests: &[PortalShortcutRequest],
            _activation: Arc<dyn Fn(ShortcutAction) + Send + Sync>,
        ) -> Result<PortalBoundShortcuts, super::super::ShortcutRegistrarError> {
            self.requests
                .lock()
                .expect("portal requests")
                .push(requests.to_vec());
            let response = self
                .responses
                .lock()
                .expect("portal responses")
                .pop_front()
                .expect("test response is queued");
            Ok(PortalBoundShortcuts::new(
                response,
                Arc::new(FakeSession(Arc::clone(&self.closed_sessions))),
            ))
        }
    }

    struct FakeSession(Arc<AtomicUsize>);

    #[async_trait]
    impl PortalSessionHandle for FakeSession {
        async fn close(&self) -> Result<(), super::PortalCloseError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn binding(key: &str) -> ShortcutBinding {
        ShortcutBinding::new(
            ShortcutAction::Screenshot,
            Some(
                ShortcutChord::new(
                    vec![ShortcutModifier::Control, ShortcutModifier::Super],
                    ShortcutKey::parse(key).expect("supported key"),
                )
                .expect("valid chord"),
            ),
        )
    }

    #[tokio::test]
    async fn portal_reports_effective_trigger_and_restores_old_session_after_rejection() {
        let original = binding("KeyP");
        let replacement = binding("KeyQ");
        let mut first_response = HashMap::new();
        first_response.insert(original.id.to_string(), "Ctrl+Super+P".to_owned());
        let mut restore_response = HashMap::new();
        restore_response.insert(original.id.to_string(), "Ctrl+Super+P".to_owned());
        let portal = Arc::new(FakePortal {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from([
                first_response,
                HashMap::new(),
                restore_response,
            ])),
            closed_sessions: Arc::new(AtomicUsize::new(0)),
        });
        let portal_port: Arc<dyn GlobalShortcutsPortal> = portal.clone();
        let registrar = PortalShortcutRegistrar::with_portal(portal_port, Arc::new(|| {}));

        let initial = registrar
            .register_available(std::slice::from_ref(&original))
            .await;
        assert!(matches!(
            initial[0].registration,
            ShortcutRegistrationState::Registered { ref effective_trigger }
                if effective_trigger == "Ctrl+Super+P"
        ));
        let error = registrar
            .replace(std::slice::from_ref(&replacement))
            .await
            .expect_err("portal rejection rejects the candidate set");

        assert!(matches!(
            error,
            super::super::ShortcutRegistrarError::Rejected { .. }
        ));
        assert_eq!(
            portal.requests.lock().expect("portal requests")[0][0].preferred_trigger,
            "CTRL+LOGO+P"
        );
        let state = registrar.state.lock().await;
        let restored = state.as_ref().expect("previous session restored");
        assert_eq!(restored.bindings, vec![original]);
        assert_eq!(restored.views, initial);
        assert_eq!(portal.closed_sessions.load(Ordering::SeqCst), 2);
        assert_eq!(portal.requests.lock().expect("portal requests").len(), 3);
    }
}
