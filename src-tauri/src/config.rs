use std::{env, time::Duration};

use thiserror::Error;

use crate::domain::{IdentifierError, ModelId, ProviderId};

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MIN_TIMEOUT_SECONDS: u64 = 5;
const MAX_TIMEOUT_SECONDS: u64 = 300;
const MAX_CONTEXT_PACK_ID_BYTES: usize = 64;

pub trait ConfigSource {
    fn get(&self, key: &'static str) -> Option<String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentConfigSource;

impl ConfigSource for EnvironmentConfigSource {
    fn get(&self, key: &'static str) -> Option<String> {
        env::var(key).ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    provider: ProviderId,
    model: ModelId,
    context_pack: String,
    request_timeout: Duration,
}

impl AppConfig {
    /// Resolves and validates the non-secret runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a required setting is absent or invalid.
    pub fn from_source(source: &impl ConfigSource) -> Result<Self, ConfigError> {
        let provider_value = required(source, "SINGULARITY_LIVE_PROVIDER")?;
        let provider = match provider_value.as_str() {
            "openrouter" => ProviderId::OpenRouter,
            _ => {
                return Err(ConfigError::UnsupportedProvider {
                    provider: provider_value,
                });
            }
        };

        let model_value = required(source, "SINGULARITY_LIVE_MODEL")?;
        let model = ModelId::new(model_value).map_err(ConfigError::InvalidModel)?;
        let context_pack = required(source, "SINGULARITY_LIVE_CONTEXT_PACK")?;
        validate_context_pack_id(&context_pack)?;
        let request_timeout = parse_timeout(source)?;

        Ok(Self {
            provider,
            model,
            context_pack,
            request_timeout,
        })
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderId {
        self.provider
    }

    #[must_use]
    pub const fn model(&self) -> &ModelId {
        &self.model
    }

    #[must_use]
    pub fn context_pack(&self) -> &str {
        &self.context_pack
    }

    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("Required setting {key} is not configured")]
    Missing { key: &'static str },
    #[error("Unsupported provider {provider}; expected openrouter")]
    UnsupportedProvider { provider: String },
    #[error("Invalid model setting: {0}")]
    InvalidModel(IdentifierError),
    #[error("Invalid context pack identifier")]
    InvalidContextPack,
    #[error(
        "SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS must be an integer from {MIN_TIMEOUT_SECONDS} to {MAX_TIMEOUT_SECONDS}"
    )]
    InvalidTimeout { value: String },
}

fn required(source: &impl ConfigSource, key: &'static str) -> Result<String, ConfigError> {
    source
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing { key })
}

fn parse_timeout(source: &impl ConfigSource) -> Result<Duration, ConfigError> {
    let Some(value) = source.get("SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS") else {
        return Ok(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
    };
    let seconds = value
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidTimeout {
            value: value.clone(),
        })?;
    if !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&seconds) {
        return Err(ConfigError::InvalidTimeout { value });
    }
    Ok(Duration::from_secs(seconds))
}

fn validate_context_pack_id(value: &str) -> Result<(), ConfigError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_CONTEXT_PACK_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(ConfigError::InvalidContextPack)
    }
}
