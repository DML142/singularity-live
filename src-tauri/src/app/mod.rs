mod application_status;
mod manual_assistance;

pub use application_status::{ApplicationStatus, BackendState, application_status};
pub use manual_assistance::{
    ManualAssistanceError, ManualAssistanceReadiness, ManualAssistanceService,
};

#[cfg(test)]
mod tests {
    use super::{BackendState, application_status};

    #[test]
    fn application_status_reports_the_product_identity_and_ready_backend() {
        let status = application_status();

        assert_eq!(status.application_name, "Singularity Live");
        assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(status.backend_state, BackendState::Ready);
    }
}
