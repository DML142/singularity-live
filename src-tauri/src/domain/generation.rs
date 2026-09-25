use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const MAX_MODEL_ID_BYTES: usize = 200;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IdentifierError {
    #[error("{kind} must not be blank")]
    Blank { kind: &'static str },
    #[error("{kind} exceeds {max_bytes} bytes")]
    TooLong {
        kind: &'static str,
        max_bytes: usize,
    },
    #[error("{kind} contains unsupported control characters")]
    ControlCharacter { kind: &'static str },
    #[error("request ID is not a valid UUID")]
    InvalidRequestId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    OpenRouter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    /// Creates a validated provider model identifier.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier is blank, too long, or contains control
    /// characters.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value, "model identifier", MAX_MODEL_ID_BYTES)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RequestId(Uuid);

impl RequestId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses a request identifier from its UUID representation.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a valid UUID.
    pub fn parse(value: &str) -> Result<Self, IdentifierError> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| IdentifierError::InvalidRequestId)
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextDocument {
    pub id: String,
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectedContext {
    pub pack_id: String,
    pub documents: Vec<ContextDocument>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
}

impl ConversationMessage {
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::User,
            content: content.into(),
        }
    }

    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ConversationRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextGenerationRequest {
    pub request_id: RequestId,
    pub provider: ProviderId,
    pub model: ModelId,
    pub selected_context: SelectedContext,
    pub system_prompt: String,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedResponse {
    pub request_id: RequestId,
    pub provider: ProviderId,
    pub model: ModelId,
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamEvent {
    Started {
        request_id: RequestId,
    },
    TextDelta {
        request_id: RequestId,
        delta: String,
    },
    Completed(CompletedResponse),
    Cancelled {
        request_id: RequestId,
    },
    Failed {
        request_id: RequestId,
        error: ProviderError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderErrorKind {
    Authentication,
    Configuration,
    InvalidRequest,
    RateLimit,
    Timeout,
    Transport,
    Provider,
    Cancellation,
    MalformedResponse,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
}

fn validate_identifier(
    value: &str,
    kind: &'static str,
    max_bytes: usize,
) -> Result<(), IdentifierError> {
    if value.trim().is_empty() {
        return Err(IdentifierError::Blank { kind });
    }
    if value.len() > max_bytes {
        return Err(IdentifierError::TooLong { kind, max_bytes });
    }
    if value.chars().any(char::is_control) {
        return Err(IdentifierError::ControlCharacter { kind });
    }
    Ok(())
}
