use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::capture::{
    CaptureError, CaptureErrorKind, CaptureOperationId, CapturePreview, CaptureTargetKind,
    ScreenCaptureService,
};

pub(crate) trait CaptureWindow: Send + Sync {
    fn hide(&self) -> Result<(), CaptureError>;
    fn restore(&self) -> Result<(), CaptureError>;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub(crate) enum HotkeyCaptureEvent {
    Preview {
        preview: CapturePreview,
    },
    Error {
        kind: CaptureErrorKind,
        message: String,
    },
}

pub(crate) trait HotkeyCaptureEventSink: Send + Sync {
    fn emit(&self, event: HotkeyCaptureEvent);
}

pub(crate) struct HotkeyCaptureCoordinator {
    captures: Arc<ScreenCaptureService>,
    window: Arc<dyn CaptureWindow>,
    events: Arc<dyn HotkeyCaptureEventSink>,
    manual_request_active: Arc<dyn Fn() -> bool + Send + Sync>,
    close_window_on_capture: Arc<dyn Fn() -> bool + Send + Sync>,
    target_kind: Arc<dyn Fn() -> CaptureTargetKind + Send + Sync>,
    active: Mutex<Option<CancellationToken>>,
}

impl HotkeyCaptureCoordinator {
    pub(crate) fn new(
        captures: Arc<ScreenCaptureService>,
        window: Arc<dyn CaptureWindow>,
        events: Arc<dyn HotkeyCaptureEventSink>,
        manual_request_active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            captures,
            window,
            events,
            manual_request_active,
            close_window_on_capture: Arc::new(|| false),
            target_kind: Arc::new(|| CaptureTargetKind::Monitor),
            active: Mutex::new(None),
        }
    }

    pub(crate) fn with_capture_preferences(
        mut self,
        close_window_on_capture: Arc<dyn Fn() -> bool + Send + Sync>,
        target_kind: Arc<dyn Fn() -> CaptureTargetKind + Send + Sync>,
    ) -> Self {
        self.close_window_on_capture = close_window_on_capture;
        self.target_kind = target_kind;
        self
    }

    pub(crate) async fn capture_from_hotkey(&self) {
        if (self.manual_request_active)() {
            self.events.emit(error_event(
                CaptureErrorKind::Busy,
                "A manual assistance request is already in progress",
            ));
            return;
        }
        let cancellation = CancellationToken::new();
        {
            let mut active = self.active_lock();
            if active.is_some() {
                self.events.emit(error_event(
                    CaptureErrorKind::Busy,
                    "A screenshot action is already in progress",
                ));
                return;
            }
            *active = Some(cancellation.clone());
        }

        let operation_id = CaptureOperationId::new();
        let hidden = (self.close_window_on_capture)();
        let target_kind = (self.target_kind)();
        let mut result = if hidden && self.window.hide().is_err() {
            Err(unavailable_error())
        } else if hidden {
            tokio::select! {
                () = cancellation.cancelled() => Err(cancelled_error()),
                () = tokio::time::sleep(Duration::from_millis(120)) => {
                    self.captures
                        .capture_target_under_pointer(target_kind, operation_id, cancellation.clone())
                        .await
                }
            }
        } else {
            self.captures
                .capture_target_under_pointer(target_kind, operation_id, cancellation.clone())
                .await
        };

        if cancellation.is_cancelled()
            && let Ok(preview) = &result
        {
            let _ = self.captures.discard(&preview.capture_id);
            result = Err(cancelled_error());
        }
        let restore_failed = hidden && self.window.restore().is_err();
        if restore_failed {
            if let Ok(preview) = &result {
                let _ = self.captures.discard(&preview.capture_id);
            }
            result = Err(unavailable_error());
        }
        *self.active_lock() = None;
        match result {
            Ok(preview) => self.events.emit(HotkeyCaptureEvent::Preview { preview }),
            Err(error) => self.events.emit(error_event(error.kind, error.message)),
        }
    }

    pub(crate) fn cancel_active(&self) {
        if let Some(active) = self.active_lock().as_ref() {
            active.cancel();
        }
    }

    fn active_lock(&self) -> MutexGuard<'_, Option<CancellationToken>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) struct TauriCaptureWindow {
    window: tauri::WebviewWindow,
}

impl TauriCaptureWindow {
    pub(crate) fn new(window: tauri::WebviewWindow) -> Self {
        Self { window }
    }
}

impl CaptureWindow for TauriCaptureWindow {
    fn hide(&self) -> Result<(), CaptureError> {
        self.window.hide().map_err(|_| unavailable_error())
    }

    fn restore(&self) -> Result<(), CaptureError> {
        self.window.unminimize().map_err(|_| unavailable_error())?;
        self.window.show().map_err(|_| unavailable_error())?;
        self.window
            .set_always_on_top(true)
            .map_err(|_| unavailable_error())?;
        self.window.set_focus().map_err(|_| unavailable_error())
    }
}

pub(crate) struct TauriHotkeyCaptureEventSink {
    app: tauri::AppHandle,
}

impl TauriHotkeyCaptureEventSink {
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl HotkeyCaptureEventSink for TauriHotkeyCaptureEventSink {
    fn emit(&self, event: HotkeyCaptureEvent) {
        let _ = tauri::Emitter::emit(&self.app, "singularity:hotkey-capture", &event);
    }
}

fn error_event(kind: CaptureErrorKind, message: &str) -> HotkeyCaptureEvent {
    HotkeyCaptureEvent::Error {
        kind,
        message: message.to_owned(),
    }
}

const fn unavailable_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Unavailable,
        "The screenshot action could not restore the application window",
    )
}

const fn cancelled_error() -> CaptureError {
    CaptureError::new(CaptureErrorKind::Cancelled, "Screen capture was cancelled")
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use xcap::image::{Rgba, RgbaImage};

    use super::{
        CaptureWindow, HotkeyCaptureCoordinator, HotkeyCaptureEvent, HotkeyCaptureEventSink,
    };
    use crate::capture::{
        CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CapturePermission,
        CaptureTarget, CaptureTargetKind, ScreenCaptureService, TransientImageStore,
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum EventSummary {
        Preview,
        Error(CaptureErrorKind),
    }

    #[derive(Default)]
    struct Trace {
        steps: Mutex<Vec<&'static str>>,
        capture_calls: AtomicUsize,
    }

    struct TestWindow(Arc<Trace>);

    impl CaptureWindow for TestWindow {
        fn hide(&self) -> Result<(), CaptureError> {
            self.0.steps.lock().expect("trace").push("hide");
            Ok(())
        }

        fn restore(&self) -> Result<(), CaptureError> {
            self.0.steps.lock().expect("trace").push("restore");
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestEvents {
        summaries: Mutex<Vec<EventSummary>>,
        trace: Option<Arc<Trace>>,
    }

    impl HotkeyCaptureEventSink for TestEvents {
        fn emit(&self, event: HotkeyCaptureEvent) {
            if let Some(trace) = &self.trace {
                trace.steps.lock().expect("trace").push("event");
            }
            let summary = match event {
                HotkeyCaptureEvent::Preview { .. } => EventSummary::Preview,
                HotkeyCaptureEvent::Error { kind, .. } => EventSummary::Error(kind),
            };
            self.summaries.lock().expect("summaries").push(summary);
        }
    }

    struct TestBackend {
        trace: Arc<Trace>,
        wait_for_cancellation: bool,
        fail_capture: bool,
    }

    #[async_trait]
    impl CaptureBackend for TestBackend {
        async fn capabilities(&self) -> CaptureCapabilities {
            CaptureCapabilities {
                targets: vec![CaptureTargetKind::Monitor],
                permission: CapturePermission::NotRequired,
                message: None,
            }
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
            self.trace.steps.lock().expect("trace").push("capture");
            self.trace.capture_calls.fetch_add(1, Ordering::SeqCst);
            if self.wait_for_cancellation {
                cancellation.cancelled().await;
                return Err(CaptureError::new(
                    CaptureErrorKind::Cancelled,
                    "Screen capture was cancelled",
                ));
            }
            if self.fail_capture {
                return Err(CaptureError::new(
                    CaptureErrorKind::Unavailable,
                    "Screen capture is unavailable",
                ));
            }
            Ok(RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255])))
        }
    }

    fn setup(
        trace: Arc<Trace>,
        wait_for_cancellation: bool,
        fail_capture: bool,
    ) -> (Arc<HotkeyCaptureCoordinator>, Arc<TestEvents>, Arc<Trace>) {
        setup_with_options(
            trace,
            Arc::new(|| false),
            wait_for_cancellation,
            fail_capture,
        )
    }

    fn setup_with_request_gate(
        trace: Arc<Trace>,
        request_active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> (Arc<HotkeyCaptureCoordinator>, Arc<TestEvents>, Arc<Trace>) {
        setup_with_options(trace, request_active, false, false)
    }

    fn setup_with_options(
        trace: Arc<Trace>,
        request_active: Arc<dyn Fn() -> bool + Send + Sync>,
        wait_for_cancellation: bool,
        fail_capture: bool,
    ) -> (Arc<HotkeyCaptureCoordinator>, Arc<TestEvents>, Arc<Trace>) {
        let backend = Arc::new(TestBackend {
            trace: Arc::clone(&trace),
            wait_for_cancellation,
            fail_capture,
        });
        let captures = Arc::new(ScreenCaptureService::new(
            backend,
            Arc::new(TransientImageStore::default()),
        ));
        let events = Arc::new(TestEvents {
            summaries: Mutex::new(Vec::new()),
            trace: Some(Arc::clone(&trace)),
        });
        let coordinator = Arc::new(
            HotkeyCaptureCoordinator::new(
                captures,
                Arc::new(TestWindow(Arc::clone(&trace))),
                events.clone(),
                request_active,
            )
            .with_capture_preferences(Arc::new(|| true), Arc::new(|| CaptureTargetKind::Monitor)),
        );
        (coordinator, events, trace)
    }

    #[tokio::test]
    async fn active_manual_request_reports_busy_without_hiding_or_capturing() {
        let trace = Arc::new(Trace::default());
        let (coordinator, events, trace) = setup_with_request_gate(trace, Arc::new(|| true));

        coordinator.capture_from_hotkey().await;

        assert_eq!(trace.capture_calls.load(Ordering::SeqCst), 0);
        assert_eq!(*trace.steps.lock().expect("trace"), vec!["event"]);
        assert_eq!(
            *events.summaries.lock().expect("summaries"),
            vec![EventSummary::Error(CaptureErrorKind::Busy)]
        );
    }

    #[tokio::test]
    async fn capture_hides_then_captures_then_restores_before_emitting_preview() {
        let trace = Arc::new(Trace::default());
        let (coordinator, events, trace) = setup(trace, false, false);

        coordinator.capture_from_hotkey().await;

        assert_eq!(
            *trace.steps.lock().expect("trace"),
            vec!["hide", "capture", "restore", "event"]
        );
        assert_eq!(
            *events.summaries.lock().expect("summaries"),
            vec![EventSummary::Preview]
        );
    }

    #[tokio::test]
    async fn failed_capture_restores_window_and_emits_safe_error() {
        let trace = Arc::new(Trace::default());
        let (coordinator, events, trace) = setup(trace, false, true);

        coordinator.capture_from_hotkey().await;

        assert_eq!(
            *trace.steps.lock().expect("trace"),
            vec!["hide", "capture", "restore", "event"]
        );
        assert_eq!(
            *events.summaries.lock().expect("summaries"),
            vec![EventSummary::Error(CaptureErrorKind::Unavailable)]
        );
    }

    #[tokio::test]
    async fn cancelling_inflight_capture_restores_window_and_clears_image() {
        let trace = Arc::new(Trace::default());
        let (coordinator, events, trace) = setup(trace, true, false);
        let capture = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.capture_from_hotkey().await })
        };
        while trace.capture_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        coordinator.cancel_active();
        capture.await.expect("capture task completes");

        assert_eq!(
            *trace.steps.lock().expect("trace"),
            vec!["hide", "capture", "restore", "event"]
        );
        assert_eq!(
            *events.summaries.lock().expect("summaries"),
            vec![EventSummary::Error(CaptureErrorKind::Cancelled)]
        );
    }

    #[tokio::test]
    async fn second_activation_while_busy_emits_busy_without_duplicate_capture() {
        let trace = Arc::new(Trace::default());
        let (coordinator, events, trace) = setup(trace, true, false);
        let capture = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.capture_from_hotkey().await })
        };
        while trace.capture_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        coordinator.capture_from_hotkey().await;
        coordinator.cancel_active();
        capture.await.expect("capture task completes");

        assert_eq!(trace.capture_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            *events.summaries.lock().expect("summaries"),
            vec![
                EventSummary::Error(CaptureErrorKind::Busy),
                EventSummary::Error(CaptureErrorKind::Cancelled),
            ]
        );
    }
}
