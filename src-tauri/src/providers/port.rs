use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::{
        CompletedResponse, ModelId, ProviderError, ProviderId, StreamEvent, TextGenerationRequest,
    },
    secrets::SecretValue,
};

pub trait StreamSink: Send + Sync {
    /// Emits one provider-independent streaming event.
    ///
    /// # Errors
    ///
    /// Returns a safe provider error when the event consumer is unavailable.
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError>;
}

#[async_trait]
pub trait TextGenerationProvider: Send + Sync {
    /// Streams one provider-independent text-generation request.
    ///
    /// # Errors
    ///
    /// Returns a classified safe error for configuration, cancellation, timeout,
    /// transport, provider, and response-format failures.
    async fn stream(
        &self,
        request: &TextGenerationRequest,
        secret: &SecretValue,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError>;
}

#[async_trait]
pub trait TextGenerationRouter: Send + Sync {
    fn provider(&self) -> ProviderId;
    fn model(&self) -> &ModelId;
    /// Checks that the configured provider can resolve its required credential.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration error when the provider cannot start a request.
    fn check_readiness(&self) -> Result<(), ProviderError>;

    /// Routes a text-generation request through the configured provider.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration or provider error.
    async fn stream(
        &self,
        request: &TextGenerationRequest,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError>;
}
