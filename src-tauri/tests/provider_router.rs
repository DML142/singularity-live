use std::{collections::HashMap, sync::Arc, time::Duration};

use singularity_live::{
    config::{AppConfig, ConfigSource},
    domain::{
        ModelId, ProviderError, ProviderErrorKind, ProviderId, RequestId, SelectedContext,
        StreamEvent, TextGenerationRequest,
    },
    providers::{ProviderRouter, StreamSink, TextGenerationRouter},
    secrets::{SecretError, SecretName, SecretStore, SecretValue},
};

struct ConfigMap(HashMap<&'static str, String>);

impl ConfigSource for ConfigMap {
    fn get(&self, key: &'static str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

struct UnusedSecretStore;

impl SecretStore for UnusedSecretStore {
    fn get(&self, name: SecretName) -> Result<SecretValue, SecretError> {
        Err(SecretError::Missing { name })
    }
}

struct NoopSink;

impl StreamSink for NoopSink {
    fn emit(&self, _event: StreamEvent) -> Result<(), ProviderError> {
        Ok(())
    }
}

fn config() -> AppConfig {
    AppConfig::from_source(&ConfigMap(HashMap::from([
        ("SINGULARITY_LIVE_PROVIDER", "openrouter".to_owned()),
        (
            "SINGULARITY_LIVE_MODEL",
            "nvidia/nemotron-3-ultra:free".to_owned(),
        ),
        ("SINGULARITY_LIVE_CONTEXT_PACK", "fictional".to_owned()),
        ("SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS", "25".to_owned()),
    ])))
    .expect("valid config")
}

#[test]
fn selects_openrouter_and_preserves_the_configured_model_and_timeout() {
    let router = ProviderRouter::new(
        config(),
        Arc::new(UnusedSecretStore),
        reqwest::Client::new(),
    );

    assert_eq!(router.provider(), ProviderId::OpenRouter);
    assert_eq!(
        router.model(),
        &ModelId::new("nvidia/nemotron-3-ultra:free").expect("model")
    );
    assert_eq!(router.request_timeout(), Duration::from_secs(25));
}

#[tokio::test]
async fn maps_missing_credentials_to_a_safe_configuration_error() {
    let router = ProviderRouter::new(
        config(),
        Arc::new(UnusedSecretStore),
        reqwest::Client::new(),
    );
    let request = TextGenerationRequest {
        request_id: RequestId::new(),
        provider: router.provider(),
        model: router.model().clone(),
        selected_context: SelectedContext::default(),
        system_prompt: "System".to_owned(),
        user_text: "User".to_owned(),
    };

    let error = router
        .stream(
            &request,
            tokio_util::sync::CancellationToken::new(),
            &NoopSink,
        )
        .await
        .expect_err("missing credential must fail before networking");

    assert_eq!(error.kind, ProviderErrorKind::Configuration);
    assert_eq!(error.to_string(), "OpenRouter credential is not configured");
    assert!(!error.to_string().contains("sensitive"));
}
