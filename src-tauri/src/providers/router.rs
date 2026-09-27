use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::{
    config::AppConfig,
    domain::{
        CompletedResponse, ModelId, ProviderError, ProviderErrorKind, ProviderId,
        TextGenerationRequest,
    },
    secrets::{SecretName, SecretStore},
};

use super::{
    GeminiAdapter, OpenRouterAdapter, StreamSink, TextGenerationProvider, TextGenerationRouter,
};

pub struct ProviderRouter {
    config: AppConfig,
    secret_store: Arc<dyn SecretStore>,
    openrouter: OpenRouterAdapter,
    gemini: GeminiAdapter,
}

impl ProviderRouter {
    #[must_use]
    pub fn new(
        config: AppConfig,
        secret_store: Arc<dyn SecretStore>,
        client: reqwest::Client,
    ) -> Self {
        let openrouter = OpenRouterAdapter::new(client.clone(), config.request_timeout());
        let gemini = GeminiAdapter::new(client, config.request_timeout());
        Self {
            config,
            secret_store,
            openrouter,
            gemini,
        }
    }

    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.config.request_timeout()
    }
}

#[async_trait]
impl TextGenerationRouter for ProviderRouter {
    fn provider(&self) -> ProviderId {
        self.config.provider()
    }

    fn model(&self) -> &ModelId {
        self.config.model()
    }

    fn check_readiness(&self) -> Result<(), ProviderError> {
        self.secret_store
            .get(secret_name(self.config.provider()))
            .map(|_| ())
            .map_err(|_| ProviderError {
                kind: ProviderErrorKind::Configuration,
                message: missing_key_message(self.config.provider()).to_owned(),
            })
    }

    async fn stream(
        &self,
        request: &TextGenerationRequest,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        if request.provider != self.provider() || request.model != *self.model() {
            return Err(ProviderError {
                kind: ProviderErrorKind::Configuration,
                message: "The request does not match the configured provider and model".to_owned(),
            });
        }
        let secret = self
            .secret_store
            .get(secret_name(self.config.provider()))
            .map_err(|_| ProviderError {
                kind: ProviderErrorKind::Configuration,
                message: missing_key_message(self.config.provider()).to_owned(),
            })?;
        match self.config.provider() {
            ProviderId::OpenRouter => {
                self.openrouter
                    .stream(request, &secret, cancellation, sink)
                    .await
            }
            ProviderId::Gemini => {
                self.gemini
                    .stream(request, &secret, cancellation, sink)
                    .await
            }
        }
    }
}

const fn secret_name(provider: ProviderId) -> SecretName {
    match provider {
        ProviderId::OpenRouter => SecretName::OpenRouterApiKey,
        ProviderId::Gemini => SecretName::GeminiApiKey,
    }
}

const fn missing_key_message(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::OpenRouter => "OpenRouter credential is not configured",
        ProviderId::Gemini => "Gemini credential is not configured",
    }
}
