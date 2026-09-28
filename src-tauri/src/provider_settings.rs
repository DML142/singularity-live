use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    config::{AppConfig, EnvironmentConfigSource},
    domain::ProviderId,
    secrets::{SecretError, SecretName, SecretStore, SecretValue},
};

const DEFAULT_PROVIDER: &str = "openai";
const DEFAULT_MODEL: &str = "gpt-6-luna";
const DEFAULT_CONTEXT_PACK: &str = "fictional-developer";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderSettingsFile {
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_context_pack")]
    context_pack: String,
    #[serde(default)]
    openai_api_key: String,
    #[serde(default)]
    openrouter_api_key: String,
    #[serde(default)]
    gemini_api_key: String,
    #[serde(default)]
    soniox_api_key: String,
}

impl Default for ProviderSettingsFile {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            context_pack: default_context_pack(),
            openai_api_key: String::new(),
            openrouter_api_key: String::new(),
            gemini_api_key: String::new(),
            soniox_api_key: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPresence {
    Configured,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKeyStatus {
    pub openai: KeyPresence,
    pub openrouter: KeyPresence,
    pub gemini: KeyPresence,
    pub soniox: KeyPresence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsView {
    pub provider: ProviderId,
    pub model: String,
    pub context_pack: String,
    pub keys: ProviderKeyStatus,
    pub file_path: String,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderSettingsError {
    #[error("Provider settings could not be read")]
    Read,
    #[error("Provider settings could not be saved")]
    Write,
    #[error("Provider settings are invalid")]
    Invalid,
    #[error("The settings file could not be opened in the file manager")]
    Open,
}

pub struct ProviderSettingsStore {
    path: PathBuf,
}

impl ProviderSettingsStore {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the provider profile and key-presence metadata without exposing key values.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is unreadable or the stored profile is invalid.
    pub fn view(&self) -> Result<ProviderSettingsView, ProviderSettingsError> {
        let settings = self.load()?;
        let config = AppConfig::from_settings(
            &settings.provider,
            settings.model.clone(),
            settings.context_pack.clone(),
            &EnvironmentConfigSource,
        )
        .map_err(|_| ProviderSettingsError::Invalid)?;
        Ok(ProviderSettingsView {
            provider: config.provider(),
            model: config.model().as_str().to_owned(),
            context_pack: config.context_pack().to_owned(),
            keys: ProviderKeyStatus {
                openai: key_presence(
                    !settings.openai_api_key.trim().is_empty()
                        || environment_key_exists(SecretName::OpenAiApiKey),
                ),
                openrouter: key_presence(
                    !settings.openrouter_api_key.trim().is_empty()
                        || environment_key_exists(SecretName::OpenRouterApiKey),
                ),
                gemini: key_presence(
                    !settings.gemini_api_key.trim().is_empty()
                        || environment_key_exists(SecretName::GeminiApiKey),
                ),
                soniox: key_presence(
                    !settings.soniox_api_key.trim().is_empty()
                        || environment_key_exists(SecretName::SonioxApiKey),
                ),
            },
            file_path: self.path.display().to_string(),
        })
    }

    /// Saves a validated provider and model while preserving keys in the local file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read/written or its profile is invalid.
    pub fn save_profile(
        &self,
        provider: ProviderId,
        model: &str,
    ) -> Result<ProviderSettingsView, ProviderSettingsError> {
        let mut settings = self.load()?;
        let provider_name = provider_name(provider);
        let config = AppConfig::from_settings(
            provider_name,
            model.to_owned(),
            settings.context_pack.clone(),
            &EnvironmentConfigSource,
        )
        .map_err(|_| ProviderSettingsError::Invalid)?;
        provider_name.clone_into(&mut settings.provider);
        config.model().as_str().clone_into(&mut settings.model);
        self.write(&settings)?;
        self.view()
    }

    /// Loads the active profile, preferring development environment settings when present.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile or request timeout is invalid.
    pub fn app_config(&self) -> Result<AppConfig, ProviderSettingsError> {
        if has_environment_profile() {
            return AppConfig::from_source(&EnvironmentConfigSource)
                .map_err(|_| ProviderSettingsError::Invalid);
        }
        let settings = self.load()?;
        AppConfig::from_settings(
            &settings.provider,
            settings.model,
            settings.context_pack,
            &EnvironmentConfigSource,
        )
        .map_err(|_| ProviderSettingsError::Invalid)
    }

    /// Creates the settings file if needed and reveals it in the native file manager.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be created or the file manager cannot launch.
    pub fn open_in_file_manager(&self) -> Result<(), ProviderSettingsError> {
        self.ensure_file()?;
        #[cfg(target_os = "windows")]
        let result = Command::new("explorer.exe")
            .arg(format!("/select,{}", self.path.display()))
            .spawn();
        #[cfg(target_os = "macos")]
        let result = Command::new("open").arg("-R").arg(&self.path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let result = Command::new("xdg-open")
            .arg(self.path.parent().unwrap_or_else(|| Path::new(".")))
            .spawn();
        result.map(|_| ()).map_err(|_| ProviderSettingsError::Open)
    }

    fn ensure_file(&self) -> Result<(), ProviderSettingsError> {
        if self.path.exists() {
            return Ok(());
        }
        self.write(&ProviderSettingsFile::default())
    }

    fn load(&self) -> Result<ProviderSettingsFile, ProviderSettingsError> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| ProviderSettingsError::Read),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ProviderSettingsFile::default())
            }
            Err(_) => Err(ProviderSettingsError::Read),
        }
    }

    fn write(&self, settings: &ProviderSettingsFile) -> Result<(), ProviderSettingsError> {
        let Some(parent) = self.path.parent() else {
            return Err(ProviderSettingsError::Write);
        };
        fs::create_dir_all(parent).map_err(|_| ProviderSettingsError::Write)?;
        let mut temporary = self.path.clone();
        temporary.set_extension("json.tmp");
        let bytes =
            serde_json::to_vec_pretty(settings).map_err(|_| ProviderSettingsError::Write)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| ProviderSettingsError::Write)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| ProviderSettingsError::Write)?;
        }
        file.write_all(&bytes)
            .map_err(|_| ProviderSettingsError::Write)?;
        file.sync_all().map_err(|_| ProviderSettingsError::Write)?;
        fs::rename(&temporary, &self.path).map_err(|_| ProviderSettingsError::Write)
    }
}

pub struct ProviderFileSecretStore {
    settings: ProviderSettingsStore,
}

impl ProviderFileSecretStore {
    #[must_use]
    pub fn new(settings: ProviderSettingsStore) -> Self {
        Self { settings }
    }
}

impl SecretStore for ProviderFileSecretStore {
    fn get(&self, name: SecretName) -> Result<SecretValue, SecretError> {
        if let Ok(environment_value) = std::env::var(name.environment_key())
            && !environment_value.trim().is_empty()
        {
            return SecretValue::new(environment_value);
        }
        let settings = self.settings.load().map_err(|_| SecretError::Invalid)?;
        let value = match name {
            SecretName::OpenAiApiKey => settings.openai_api_key,
            SecretName::OpenRouterApiKey => settings.openrouter_api_key,
            SecretName::GeminiApiKey => settings.gemini_api_key,
            SecretName::SonioxApiKey => settings.soniox_api_key,
        };
        if !value.trim().is_empty() {
            return SecretValue::new(value);
        }
        Err(SecretError::Missing { name })
    }
}

fn environment_key_exists(name: SecretName) -> bool {
    std::env::var(name.environment_key()).is_ok_and(|value| !value.trim().is_empty())
}

const fn key_presence(configured: bool) -> KeyPresence {
    if configured {
        KeyPresence::Configured
    } else {
        KeyPresence::Missing
    }
}

const fn provider_name(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::OpenAi => "openai",
        ProviderId::OpenRouter => "openrouter",
        ProviderId::Gemini => "gemini",
    }
}

fn has_environment_profile() -> bool {
    [
        "SINGULARITY_LIVE_PROVIDER",
        "SINGULARITY_LIVE_MODEL",
        "SINGULARITY_LIVE_CONTEXT_PACK",
    ]
    .into_iter()
    .any(|key| std::env::var_os(key).is_some())
}

fn default_provider() -> String {
    DEFAULT_PROVIDER.to_owned()
}

fn default_model() -> String {
    DEFAULT_MODEL.to_owned()
}

fn default_context_pack() -> String {
    DEFAULT_CONTEXT_PACK.to_owned()
}
