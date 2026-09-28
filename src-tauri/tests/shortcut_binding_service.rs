use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use singularity_live::shortcuts::{
    AtomicConfigWriter, ShortcutAction, ShortcutBinding, ShortcutBindingService,
    ShortcutBindingView, ShortcutChord, ShortcutConfigStore, ShortcutKey, ShortcutModifier,
    ShortcutPlatform, ShortcutRegistrar, ShortcutRegistrarError, ShortcutRegistrationState,
    ShortcutServiceError,
};
use tempfile::NamedTempFile;

#[derive(Default)]
struct RegistrarState {
    active: Vec<ShortcutBinding>,
    rejected_chords: HashSet<String>,
    effective_triggers: HashMap<String, String>,
}

#[derive(Default)]
struct FakeRegistrar(Mutex<RegistrarState>);

impl FakeRegistrar {
    fn reject(&self, chord: &ShortcutChord) {
        self.0
            .lock()
            .expect("registrar lock")
            .rejected_chords
            .insert(chord.canonical());
    }

    fn set_effective_trigger(&self, requested: &ShortcutChord, actual: &str) {
        self.0
            .lock()
            .expect("registrar lock")
            .effective_triggers
            .insert(requested.canonical(), actual.to_owned());
    }

    fn active(&self) -> Vec<ShortcutBinding> {
        self.0.lock().expect("registrar lock").active.clone()
    }
}

#[async_trait]
impl ShortcutRegistrar for FakeRegistrar {
    async fn register_available(&self, bindings: &[ShortcutBinding]) -> Vec<ShortcutBindingView> {
        let (statuses, active) = self.evaluate(bindings);
        self.0.lock().expect("registrar lock").active = active;
        statuses
    }

    async fn replace(
        &self,
        bindings: &[ShortcutBinding],
    ) -> Result<Vec<ShortcutBindingView>, ShortcutRegistrarError> {
        let (statuses, active) = self.evaluate(bindings);
        let failures = statuses
            .iter()
            .filter_map(|view| match &view.registration {
                ShortcutRegistrationState::Failed { message } => {
                    Some(singularity_live::shortcuts::BindingRegistrationFailure {
                        binding_id: view.binding.id,
                        message: message.clone(),
                    })
                }
                ShortcutRegistrationState::Registered { .. }
                | ShortcutRegistrationState::Unbound => None,
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(ShortcutRegistrarError::Rejected { failures });
        }
        self.0.lock().expect("registrar lock").active = active;
        Ok(statuses)
    }

    async fn unregister_all(&self) -> Result<(), ShortcutRegistrarError> {
        self.0.lock().expect("registrar lock").active.clear();
        Ok(())
    }
}

impl FakeRegistrar {
    fn evaluate(
        &self,
        bindings: &[ShortcutBinding],
    ) -> (Vec<ShortcutBindingView>, Vec<ShortcutBinding>) {
        let state = self.0.lock().expect("registrar lock");
        let mut active = Vec::new();
        let statuses = bindings
            .iter()
            .map(|binding| {
                let registration = match &binding.chord {
                    None => ShortcutRegistrationState::Unbound,
                    Some(chord) if state.rejected_chords.contains(&chord.canonical()) => {
                        ShortcutRegistrationState::Failed {
                            message: "The operating system rejected this shortcut".to_owned(),
                        }
                    }
                    Some(chord) => {
                        active.push(binding.clone());
                        ShortcutRegistrationState::Registered {
                            effective_trigger: state
                                .effective_triggers
                                .get(&chord.canonical())
                                .cloned()
                                .unwrap_or_else(|| chord.canonical()),
                        }
                    }
                };
                ShortcutBindingView {
                    binding: binding.clone(),
                    registration,
                }
            })
            .collect();
        (statuses, active)
    }
}

struct ToggleAtomicWriter {
    fail: AtomicBool,
}

impl AtomicConfigWriter for ToggleAtomicWriter {
    fn replace_atomically(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("simulated write error"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(contents)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

fn chord(modifier: ShortcutModifier, key: &str) -> ShortcutChord {
    ShortcutChord::new(
        vec![modifier],
        ShortcutKey::parse(key).expect("supported key"),
    )
    .expect("valid chord")
}

#[tokio::test]
async fn startup_keeps_valid_bindings_and_reports_os_rejected_rows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Arc::new(ShortcutConfigStore::new(
        directory.path().join("shortcuts.json"),
        ShortcutPlatform::Linux,
    ));
    let defaults = store.load().expect("defaults");
    let rejected = ShortcutBinding::new(
        ShortcutAction::Screenshot,
        Some(chord(ShortcutModifier::Control, "KeyS")),
    );
    let mut configured = defaults.clone();
    configured.push(rejected.clone());
    store.save(&configured).expect("persist bindings");

    let registrar = Arc::new(FakeRegistrar::default());
    registrar.reject(rejected.chord.as_ref().expect("binding chord"));
    registrar.set_effective_trigger(
        defaults[0].chord.as_ref().expect("default chord"),
        "Control+Super+P",
    );
    let service = ShortcutBindingService::new(store, registrar.clone());

    let states = service.initialize().await.expect("initialize shortcuts");

    assert!(matches!(
        states[0].registration,
        ShortcutRegistrationState::Registered { ref effective_trigger }
            if effective_trigger == "Control+Super+P"
    ));
    let rejected_state = states
        .iter()
        .find(|state| state.binding.id == rejected.id)
        .expect("rejected binding state");
    assert!(matches!(
        rejected_state.registration,
        ShortcutRegistrationState::Failed { .. }
    ));
    assert_eq!(
        registrar.active(),
        defaults
            .iter()
            .filter(|binding| binding.chord.is_some())
            .cloned()
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn rejected_update_restores_previous_runtime_bindings_and_persisted_config() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Arc::new(ShortcutConfigStore::new(
        directory.path().join("shortcuts.json"),
        ShortcutPlatform::Linux,
    ));
    let original = store.load().expect("defaults");
    store.save(&original).expect("persist defaults");
    let registrar = Arc::new(FakeRegistrar::default());
    let service = ShortcutBindingService::new(store.clone(), registrar.clone());
    service.initialize().await.expect("initialize");
    let conflicting_chord = chord(ShortcutModifier::Control, "KeyQ");
    registrar.reject(&conflicting_chord);
    let candidate = vec![ShortcutBinding::new(
        ShortcutAction::Screenshot,
        Some(conflicting_chord),
    )];

    let error = service
        .update_bindings(candidate)
        .await
        .expect_err("OS conflict rejects the update");

    assert!(matches!(
        error,
        ShortcutServiceError::RegistrationRejected { .. }
    ));
    assert_eq!(store.load().expect("prior config remains"), original);
    assert_eq!(
        registrar.active(),
        original
            .iter()
            .filter(|binding| binding.chord.is_some())
            .cloned()
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn persistence_failure_rolls_back_newly_registered_bindings() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let writer = Arc::new(ToggleAtomicWriter {
        fail: AtomicBool::new(false),
    });
    let store = Arc::new(ShortcutConfigStore::with_writer(
        directory.path().join("shortcuts.json"),
        ShortcutPlatform::Linux,
        writer.clone(),
    ));
    let registrar = Arc::new(FakeRegistrar::default());
    let service = ShortcutBindingService::new(store.clone(), registrar.clone());
    service.initialize().await.expect("initialize");
    let replacement = vec![ShortcutBinding::new(
        ShortcutAction::Screenshot,
        Some(chord(ShortcutModifier::Control, "KeyQ")),
    )];
    let prior_status = service
        .update_bindings(replacement)
        .await
        .expect("first update persists");
    let persisted = store.load().expect("read persisted settings");
    let runtime = registrar.active();
    writer.fail.store(true, Ordering::SeqCst);
    let another = vec![ShortcutBinding::new(
        ShortcutAction::Screenshot,
        Some(chord(ShortcutModifier::Control, "KeyR")),
    )];

    let error = service
        .update_bindings(another)
        .await
        .expect_err("storage error rejects update");

    assert!(matches!(error, ShortcutServiceError::Configuration));
    assert_eq!(store.load().expect("prior config remains"), persisted);
    assert_eq!(registrar.active(), runtime);
    assert_eq!(service.snapshot().await, prior_status);
}
