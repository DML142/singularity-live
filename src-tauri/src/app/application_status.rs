use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendState {
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationStatus {
    pub application_name: &'static str,
    pub version: &'static str,
    pub backend_state: BackendState,
}

#[must_use]
pub const fn application_status() -> ApplicationStatus {
    ApplicationStatus {
        application_name: "Singularity Live",
        version: env!("CARGO_PKG_VERSION"),
        backend_state: BackendState::Ready,
    }
}
