mod backend;
mod image;
#[cfg(target_os = "linux")]
mod portal;
mod store;

pub use backend::{
    CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CaptureOperationId,
    CapturePermission, CaptureTarget, CaptureTargetKind, platform_capture_backend,
};
pub use image::{CropRect, PreparedImage, prepare_image};
pub use store::{CaptureId, CapturePreview, TransientImageStore};

use std::sync::{Arc, Mutex, MutexGuard};

use tokio_util::sync::CancellationToken;
pub struct ScreenCaptureService {
    backend: Arc<dyn CaptureBackend>,
    images: Arc<TransientImageStore>,
    active: Mutex<Option<ActiveCapture>>,
}

struct ActiveCapture {
    operation_id: CaptureOperationId,
    cancellation: CancellationToken,
}

impl ScreenCaptureService {
    /// Creates a screen-capture service with a platform adapter and transient image store.
    #[must_use]
    pub fn new(backend: Arc<dyn CaptureBackend>, images: Arc<TransientImageStore>) -> Self {
        Self {
            backend,
            images,
            active: Mutex::new(None),
        }
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
        if !self.backend.capabilities().await.supports(kind) {
            return Err(CaptureError::new(
                CaptureErrorKind::Unsupported,
                "This screen or window capture type is unavailable",
            ));
        }
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
        operation_id: CaptureOperationId,
    ) -> Result<CapturePreview, CaptureError> {
        let cancellation = self.begin_capture(operation_id)?;
        self.images.clear();
        let mut result = self.capture_target(target_id, cancellation.clone()).await;
        if cancellation.is_cancelled() {
            result = Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        if result.is_err() {
            self.images.clear();
        }
        self.finish_capture(operation_id);
        result
    }

    /// Cancels the active screenshot operation when the identifier matches.
    ///
    /// # Errors
    ///
    /// Returns a no-matching-capture error when this operation is no longer active.
    pub fn cancel_capture(&self, operation_id: CaptureOperationId) -> Result<(), CaptureError> {
        let active = self.active_lock();
        let Some(current) = active.as_ref() else {
            return Err(no_matching_capture_error());
        };
        if current.operation_id != operation_id {
            return Err(no_matching_capture_error());
        }
        current.cancellation.cancel();
        Ok(())
    }

    /// Cancels any active screenshot operation and clears its transient preview.
    pub fn clear(&self) {
        if let Some(active) = self.active_lock().as_ref() {
            active.cancellation.cancel();
        }
        self.images.clear();
    }

    fn begin_capture(
        &self,
        operation_id: CaptureOperationId,
    ) -> Result<CancellationToken, CaptureError> {
        let mut active = self.active_lock();
        if active.is_some() {
            return Err(CaptureError::new(
                CaptureErrorKind::Busy,
                "A screen capture is already in progress",
            ));
        }
        let cancellation = CancellationToken::new();
        *active = Some(ActiveCapture {
            operation_id,
            cancellation: cancellation.clone(),
        });
        Ok(cancellation)
    }

    fn finish_capture(&self, operation_id: CaptureOperationId) {
        let mut active = self.active_lock();
        if active
            .as_ref()
            .is_some_and(|current| current.operation_id == operation_id)
        {
            *active = None;
        }
    }

    async fn capture_target(
        &self,
        target_id: &str,
        cancellation: CancellationToken,
    ) -> Result<CapturePreview, CaptureError> {
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
        if !capabilities.supports(target.kind) {
            return Err(CaptureError::new(
                CaptureErrorKind::Unsupported,
                "This screen or window capture type is unavailable",
            ));
        }
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        let pixels = self.backend.capture(&target, cancellation.clone()).await?;
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        let prepared = prepare_image(pixels)?;
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        self.images.insert(prepared)
    }

    fn active_lock(&self) -> MutexGuard<'_, Option<ActiveCapture>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

const fn no_matching_capture_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::NoMatchingCapture,
        "No active screen capture matches that identifier",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use xcap::image::{Rgba, RgbaImage};

    use super::{
        CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CaptureOperationId,
        CapturePermission, CaptureTarget, CaptureTargetKind, ScreenCaptureService,
        TransientImageStore, prepare_image,
    };

    struct FakeBackend {
        capabilities: CaptureCapabilities,
        capture_calls: AtomicUsize,
        wait_for_cancellation: bool,
    }

    impl FakeBackend {
        fn new(capabilities: CaptureCapabilities) -> Self {
            Self {
                capabilities,
                capture_calls: AtomicUsize::new(0),
                wait_for_cancellation: false,
            }
        }

        fn cancellable(capabilities: CaptureCapabilities) -> Self {
            Self {
                capabilities,
                capture_calls: AtomicUsize::new(0),
                wait_for_cancellation: true,
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
            cancellation: tokio_util::sync::CancellationToken,
        ) -> Result<RgbaImage, CaptureError> {
            self.capture_calls.fetch_add(1, Ordering::SeqCst);
            if self.wait_for_cancellation {
                cancellation.cancelled().await;
                return Err(CaptureError::new(
                    CaptureErrorKind::Cancelled,
                    "Screen capture was cancelled",
                ));
            }
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
            .capture("monitor-1", CaptureOperationId::new())
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
            .capture("monitor-1", CaptureOperationId::new())
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
            .capture("monitor-1", CaptureOperationId::new())
            .await
            .expect_err("denied permission is rejected");

        assert_eq!(error.kind, CaptureErrorKind::PermissionDenied);
        assert_eq!(backend.capture_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancelling_capture_releases_prior_preview_and_active_operation() {
        let store = std::sync::Arc::new(TransientImageStore::default());
        let previous = store
            .insert(
                prepare_image(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
                    .expect("previous preview is prepared"),
            )
            .expect("previous preview is stored");
        let backend = std::sync::Arc::new(FakeBackend::cancellable(capabilities()));
        let service =
            std::sync::Arc::new(ScreenCaptureService::new(backend.clone(), store.clone()));
        let operation_id = CaptureOperationId::new();
        let capture_service = service.clone();
        let task =
            tokio::spawn(async move { capture_service.capture("monitor-1", operation_id).await });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while backend.capture_calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("capture backend starts");

        service
            .cancel_capture(operation_id)
            .expect("active capture cancels");
        let error = task
            .await
            .expect("capture task joins")
            .expect_err("cancelled capture has no preview");

        assert_eq!(error.kind, CaptureErrorKind::Cancelled);
        assert_eq!(
            store
                .preview(&previous.capture_id)
                .expect_err("old preview was cleared before capture")
                .kind,
            CaptureErrorKind::ImageExpired
        );
        assert_eq!(
            service
                .cancel_capture(operation_id)
                .expect_err("completed operation is no longer active")
                .kind,
            CaptureErrorKind::NoMatchingCapture
        );
    }

    #[test]
    fn clearing_capture_service_removes_its_preview_for_session_reset() {
        let store = std::sync::Arc::new(TransientImageStore::default());
        let preview = store
            .insert(
                prepare_image(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
                    .expect("preview is prepared"),
            )
            .expect("preview is stored");
        let service = ScreenCaptureService::new(
            std::sync::Arc::new(FakeBackend::new(capabilities())),
            store.clone(),
        );

        service.clear();

        assert_eq!(
            store
                .preview(&preview.capture_id)
                .expect_err("session reset clears the preview")
                .kind,
            CaptureErrorKind::ImageExpired
        );
    }
}
