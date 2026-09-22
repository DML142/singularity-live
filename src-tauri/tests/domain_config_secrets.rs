use std::{collections::HashMap, time::Duration};

use singularity_live::{
    config::{AppConfig, ConfigError, ConfigSource},
    domain::{ModelId, ProviderId, RequestId},
    secrets::{SecretError, SecretName, SecretStore, SecretValue},
};

#[derive(Default)]
struct MapConfigSource {
    values: HashMap<&'static str, String>,
}

impl MapConfigSource {
    fn valid() -> Self {
        Self {
            values: HashMap::from([
                ("SINGULARITY_LIVE_PROVIDER", "openrouter".to_owned()),
                ("SINGULARITY_LIVE_MODEL", "openrouter/free".to_owned()),
                (
                    "SINGULARITY_LIVE_CONTEXT_PACK",
                    "fictional-developer".to_owned(),
                ),
            ]),
        }
    }

    fn without(mut self, key: &'static str) -> Self {
        self.values.remove(key);
        self
    }
}

impl ConfigSource for MapConfigSource {
    fn get(&self, key: &'static str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

#[test]
fn configuration_requires_every_named_setting() {
    for key in [
        "SINGULARITY_LIVE_PROVIDER",
        "SINGULARITY_LIVE_MODEL",
        "SINGULARITY_LIVE_CONTEXT_PACK",
    ] {
        let error = AppConfig::from_source(&MapConfigSource::valid().without(key))
            .expect_err("missing configuration must fail");

        assert_eq!(error, ConfigError::Missing { key });
        assert!(error.to_string().contains(key));
    }
}

#[test]
fn configuration_accepts_only_openrouter() {
    let mut source = MapConfigSource::valid();
    source
        .values
        .insert("SINGULARITY_LIVE_PROVIDER", "openai".to_owned());

    let error = AppConfig::from_source(&source).expect_err("unsupported provider must fail");

    assert_eq!(
        error,
        ConfigError::UnsupportedProvider {
            provider: "openai".to_owned(),
        }
    );
}

#[test]
fn configuration_uses_a_bounded_timeout() {
    let default = AppConfig::from_source(&MapConfigSource::valid()).expect("valid config");
    assert_eq!(default.request_timeout(), Duration::from_secs(60));

    let mut below_minimum = MapConfigSource::valid();
    below_minimum
        .values
        .insert("SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS", "4".to_owned());
    assert!(matches!(
        AppConfig::from_source(&below_minimum),
        Err(ConfigError::InvalidTimeout { .. })
    ));

    let mut above_maximum = MapConfigSource::valid();
    above_maximum
        .values
        .insert("SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS", "301".to_owned());
    assert!(matches!(
        AppConfig::from_source(&above_maximum),
        Err(ConfigError::InvalidTimeout { .. })
    ));
}

#[test]
fn configuration_produces_typed_provider_model_and_pack_values() {
    let config = AppConfig::from_source(&MapConfigSource::valid()).expect("valid config");

    assert_eq!(config.provider(), ProviderId::OpenRouter);
    assert_eq!(
        config.model(),
        &ModelId::new("openrouter/free").expect("model")
    );
    assert_eq!(config.context_pack(), "fictional-developer");
}

#[test]
fn identifiers_reject_blank_values_and_request_ids_are_stable() {
    assert!(ModelId::new("   ").is_err());

    let request_id = RequestId::new();
    let parsed = RequestId::parse(&request_id.to_string()).expect("request ID round trips");
    assert_eq!(parsed, request_id);
}

#[test]
fn secret_debug_output_is_redacted() {
    let secret = SecretValue::new("sensitive-value".to_owned()).expect("valid test secret");

    assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
    assert!(!format!("{secret:?}").contains("sensitive-value"));
}

#[test]
fn missing_secret_error_names_only_the_variable() {
    struct EmptyStore;

    impl SecretStore for EmptyStore {
        fn get(&self, name: SecretName) -> Result<SecretValue, SecretError> {
            Err(SecretError::Missing { name })
        }
    }

    let error = EmptyStore
        .get(SecretName::OpenRouterApiKey)
        .expect_err("missing secret must fail");

    assert_eq!(
        error.to_string(),
        "Required credential OPENROUTER_API_KEY is not configured"
    );
}
