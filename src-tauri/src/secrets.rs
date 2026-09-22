use std::{env, fmt};

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretName {
    OpenRouterApiKey,
}

impl SecretName {
    #[must_use]
    pub const fn environment_key(self) -> &'static str {
        match self {
            Self::OpenRouterApiKey => "OPENROUTER_API_KEY",
        }
    }
}

pub struct SecretValue(String);

impl SecretValue {
    /// Wraps a non-empty credential without exposing it through formatting traits.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential is blank.
    pub fn new(value: String) -> Result<Self, SecretError> {
        if value.trim().is_empty() {
            return Err(SecretError::Invalid);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

pub trait SecretStore: Send + Sync {
    /// Resolves a credential by its domain name.
    ///
    /// # Errors
    ///
    /// Returns a safe error when the credential is missing or invalid.
    fn get(&self, name: SecretName) -> Result<SecretValue, SecretError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentSecretStore;

impl SecretStore for EnvironmentSecretStore {
    fn get(&self, name: SecretName) -> Result<SecretValue, SecretError> {
        let value = env::var(name.environment_key()).map_err(|_| SecretError::Missing { name })?;
        SecretValue::new(value)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SecretError {
    #[error("Required credential {} is not configured", name.environment_key())]
    Missing { name: SecretName },
    #[error("Configured credential is empty")]
    Invalid,
}
