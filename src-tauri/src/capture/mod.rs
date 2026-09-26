mod backend;
mod image;
#[cfg(target_os = "linux")]
mod portal;
mod store;

pub use backend::{
    CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CapturePermission,
    CaptureTarget, CaptureTargetKind, platform_capture_backend,
};
pub use image::{CropRect, PreparedImage, prepare_image};
pub use store::{CaptureId, CapturePreview, TransientImageStore};

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
pub struct ScreenCaptureService {
    backend: Arc<dyn CaptureBackend>,
    images: Arc<TransientImageStore>,
}

impl ScreenCaptureService {
    /// Creates a screen-capture service with a platform adapter and transient image store.
    #[must_use]
    pub fn new(backend: Arc<dyn CaptureBackend>, images: Arc<TransientImageStore>) -> Self {
        Self { backend, images }
    }

    pub async fn capabilities(&self) -> CaptureCapabilities {
        self.backend.capabilities().await
    }

    /// Lists available targets of the requested type without capturing them.
    ///
    /// # Errors
    ///
    /// Returns an unsupported or unavailable error when the platform cannot list targets.
    pub async fn targets(
        &self,
        kind: CaptureTargetKind,
    ) -> Result<Vec<CaptureTarget>, CaptureError> {
        self.backend.targets(kind).await
    }

    /// Captures and prepares one target after an explicit caller action.
    ///
    /// # Errors
    ///
    /// Returns a capability, permission, target, preparation, availability, or cancellation
    /// error. A failed capture leaves no current image in the transient store.
    pub async fn capture(
        &self,
        target_id: &str,
        cancellation: CancellationToken,
    ) -> Result<CapturePreview, CaptureError> {
        self.images.clear();
        let capabilities = self.backend.capabilities().await;
        if !capabilities.is_supported() {
            return Err(CaptureError::new(
                CaptureErrorKind::Unsupported,
                "Screen capture is unavailable on this desktop",
            ));
        }
        if capabilities.permission == CapturePermission::Denied {
            return Err(CaptureError::new(
                CaptureErrorKind::PermissionDenied,
                "Screen capture permission was denied",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        let target = self.backend.target_by_id(target_id).await?;
        let pixels = self.backend.capture(&target, cancellation.clone()).await?;
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        let prepared = prepare_image(pixels)?;
        self.images.insert(prepared)
    }

    /// Replaces the stored image with a validated crop of its prepared pixels.
    ///
    /// # Errors
    ///
    /// Returns an expired-image or invalid-region error when the identifier or rectangle is
    /// invalid.
    pub fn crop(
        &self,
        capture_id: &CaptureId,
        rect: CropRect,
    ) -> Result<CapturePreview, CaptureError> {
        self.images.crop(capture_id, rect)
    }

    /// Removes the current image when its opaque identifier matches.
    ///
    /// # Errors
    ///
    /// Returns an expired-image error when the identifier is stale or does not match.
    pub fn discard(&self, capture_id: &CaptureId) -> Result<(), CaptureError> {
        self.images.discard(capture_id)
    }

    #[must_use]
    pub fn store(&self) -> Arc<TransientImageStore> {
        Arc::clone(&self.images)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use xcap::image::{Rgba, RgbaImage};

    use super::{
        CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CapturePermission,
        CaptureTarget, CaptureTargetKind, ScreenCaptureService, TransientImageStore,
    };

    struct FakeBackend {
        capabilities: CaptureCapabilities,
        capture_calls: AtomicUsize,
    }

    impl FakeBackend {
        fn new(capabilities: CaptureCapabilities) -> Self {
            Self {
                capabilities,
                capture_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl CaptureBackend for FakeBackend {
        async fn capabilities(&self) -> CaptureCapabilities {
            self.capabilities.clone()
        }

        async fn targets(
            &self,
            kind: CaptureTargetKind,
        ) -> Result<Vec<CaptureTarget>, CaptureError> {
            Ok(vec![CaptureTarget {
                id: "monitor-1".to_owned(),
                label: "Monitor 1".to_owned(),
                kind,
            }])
        }

        async fn capture(
            &self,
            _target: &CaptureTarget,
            _cancellation: tokio_util::sync::CancellationToken,
        ) -> Result<RgbaImage, CaptureError> {
            self.capture_calls.fetch_add(1, Ordering::SeqCst);
            Ok(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
        }
    }

    fn capabilities() -> CaptureCapabilities {
        CaptureCapabilities {
            targets: vec![CaptureTargetKind::Monitor, CaptureTargetKind::Window],
            permission: CapturePermission::Unknown,
            message: None,
        }
    }

    #[tokio::test]
    async fn capability_and_target_queries_never_capture() {
        let backend = std::sync::Arc::new(FakeBackend::new(capabilities()));
        let service = ScreenCaptureService::new(
            backend.clone(),
            std::sync::Arc::new(TransientImageStore::default()),
        );

        assert!(service.capabilities().await.is_supported());
        assert_eq!(
            service
                .targets(CaptureTargetKind::Monitor)
                .await
                .expect("targets are available")
                .len(),
            1
        );
        assert_eq!(backend.capture_calls.load(Ordering::SeqCst), 0);

        let preview = service
            .capture("monitor-1", tokio_util::sync::CancellationToken::new())
            .await
            .expect("explicit capture succeeds");
        assert_eq!(backend.capture_calls.load(Ordering::SeqCst), 1);
        assert!(preview.data_url.starts_with("data:image/png;base64,"));
    }

    #[tokio::test]
    async fn unsupported_capability_blocks_capture_before_backend_call() {
        let backend = std::sync::Arc::new(FakeBackend::new(CaptureCapabilities {
            targets: Vec::new(),
            permission: CapturePermission::Unknown,
            message: Some("Unsupported".to_owned()),
        }));
        let service = ScreenCaptureService::new(
            backend.clone(),
            std::sync::Arc::new(TransientImageStore::default()),
        );

        let error = service
            .capture("monitor-1", tokio_util::sync::CancellationToken::new())
            .await
            .expect_err("unsupported capture is rejected");

        assert_eq!(error.kind, CaptureErrorKind::Unsupported);
        assert_eq!(backend.capture_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn denied_permission_blocks_capture_before_backend_call() {
        let backend = std::sync::Arc::new(FakeBackend::new(CaptureCapabilities {
            permission: CapturePermission::Denied,
            ..capabilities()
        }));
        let service = ScreenCaptureService::new(
            backend.clone(),
            std::sync::Arc::new(TransientImageStore::default()),
        );

        let error = service
            .capture("monitor-1", tokio_util::sync::CancellationToken::new())
            .await
            .expect_err("denied permission is rejected");

        assert_eq!(error.kind, CaptureErrorKind::PermissionDenied);
        assert_eq!(backend.capture_calls.load(Ordering::SeqCst), 0);
    }
}
