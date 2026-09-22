use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use singularity_live::{
    app::{ManualAssistanceError, ManualAssistanceReadiness, ManualAssistanceService},
    domain::{
        CompletedResponse, ModelId, ProviderError, ProviderErrorKind, ProviderId, RequestId,
        StreamEvent, TextGenerationRequest,
    },
    providers::{StreamSink, TextGenerationRouter},
};
use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};
use tokio_util::sync::CancellationToken;

const MANIFEST: &str = r"schema_version: 1
id: fictional
name: Fictional
documents:
  - id: style
    title: Style
    path: style.md
    always_include: true
    keywords: []
  - id: rust
    title: Rust
    path: rust.md
    always_include: false
    keywords: [rust]
";

struct PackFixture {
    root: TempDir,
}

impl PackFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary pack");
        fs::write(root.path().join("manifest.yaml"), MANIFEST).expect("manifest");
        fs::write(root.path().join("style.md"), "Answer briefly.").expect("style");
        fs::write(root.path().join("rust.md"), "Uses Rust daily.").expect("rust");
        Self { root }
    }

    fn path(&self) -> PathBuf {
        self.root.path().to_owned()
    }
}

#[derive(Clone)]
enum RouterBehavior {
    Success,
    WaitForCancellation,
    Failure(ProviderErrorKind),
}

struct FakeRouter {
    behavior: RouterBehavior,
    ready: bool,
    captured: Arc<Mutex<Vec<TextGenerationRequest>>>,
}

impl FakeRouter {
    fn new(behavior: RouterBehavior) -> Self {
        Self {
            behavior,
            ready: true,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn unconfigured() -> Self {
        Self {
            behavior: RouterBehavior::Success,
            ready: false,
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl TextGenerationRouter for FakeRouter {
    fn provider(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    fn model(&self) -> &ModelId {
        static MODEL: std::sync::OnceLock<ModelId> = std::sync::OnceLock::new();
        MODEL.get_or_init(|| ModelId::new("openrouter/free").expect("model"))
    }

    fn check_readiness(&self) -> Result<(), ProviderError> {
        if self.ready {
            Ok(())
        } else {
            Err(ProviderError {
                kind: ProviderErrorKind::Configuration,
                message: "OpenRouter credential is not configured".to_owned(),
            })
        }
    }

    async fn stream(
        &self,
        request: &TextGenerationRequest,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        self.captured
            .lock()
            .expect("captured request lock")
            .push(request.clone());
        match self.behavior {
            RouterBehavior::Success => {
                sink.emit(StreamEvent::TextDelta {
                    request_id: request.request_id,
                    delta: "A streamed answer".to_owned(),
                })?;
                Ok(CompletedResponse {
                    request_id: request.request_id,
                    provider: request.provider,
                    model: request.model.clone(),
                    usage: None,
                })
            }
            RouterBehavior::WaitForCancellation => {
                cancellation.cancelled().await;
                Err(ProviderError {
                    kind: ProviderErrorKind::Cancellation,
                    message: "The request was cancelled".to_owned(),
                })
            }
            RouterBehavior::Failure(kind) => Err(ProviderError {
                kind,
                message: "Safe provider failure".to_owned(),
            }),
        }
    }
}

struct ChannelSink {
    sender: mpsc::UnboundedSender<StreamEvent>,
}

impl StreamSink for ChannelSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        self.sender.send(event).map_err(|_| ProviderError {
            kind: ProviderErrorKind::Cancellation,
            message: "Event receiver is unavailable".to_owned(),
        })
    }
}

fn service(
    pack: &PackFixture,
    router: Arc<dyn TextGenerationRouter>,
) -> Arc<ManualAssistanceService> {
    Arc::new(ManualAssistanceService::configured(
        pack.path(),
        "fictional".to_owned(),
        router,
    ))
}

fn channel_sink() -> (Arc<ChannelSink>, mpsc::UnboundedReceiver<StreamEvent>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Arc::new(ChannelSink { sender }), receiver)
}

async fn next_event(receiver: &mut mpsc::UnboundedReceiver<StreamEvent>) -> StreamEvent {
    timeout(std::time::Duration::from_secs(1), receiver.recv())
        .await
        .expect("event timeout")
        .expect("event channel open")
}

#[test]
fn readiness_reports_configuration_and_context_failures_safely() {
    let pack = PackFixture::new();
    let ready = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    assert!(matches!(
        ready.readiness(),
        ManualAssistanceReadiness::Ready {
            provider: ProviderId::OpenRouter,
            ..
        }
    ));

    let unconfigured = service(&pack, Arc::new(FakeRouter::unconfigured()));
    assert_eq!(
        unconfigured.readiness(),
        ManualAssistanceReadiness::Unconfigured {
            message: "OpenRouter credential is not configured".to_owned(),
        }
    );

    let missing_context = Arc::new(ManualAssistanceService::configured(
        pack.path().join("missing"),
        "missing".to_owned(),
        Arc::new(FakeRouter::new(RouterBehavior::Success)),
    ));
    assert!(matches!(
        missing_context.readiness(),
        ManualAssistanceReadiness::Unconfigured { .. }
    ));
}

#[tokio::test]
async fn unconfigured_service_starts_safely_and_rejects_requests() {
    let service = Arc::new(ManualAssistanceService::unconfigured(
        "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
    ));
    let (sink, _) = channel_sink();

    assert_eq!(
        service.readiness(),
        ManualAssistanceReadiness::Unconfigured {
            message: "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
        }
    );
    assert_eq!(
        service.start("Hello".to_owned(), sink).await,
        Err(ManualAssistanceError::NotConfigured {
            message: "Required setting SINGULARITY_LIVE_PROVIDER is not configured".to_owned(),
        })
    );
}

#[tokio::test]
async fn validates_manual_text_before_starting() {
    let pack = PackFixture::new();
    let service = service(&pack, Arc::new(FakeRouter::new(RouterBehavior::Success)));
    let (sink, _) = channel_sink();

    assert_eq!(
        service.start("   ".to_owned(), sink.clone()).await,
        Err(ManualAssistanceError::EmptyInput)
    );
    assert_eq!(
        service.start("x".repeat(16 * 1024 + 1), sink).await,
        Err(ManualAssistanceError::InputTooLarge)
    );
}

#[tokio::test]
async fn loads_selected_context_and_forwards_a_successful_stream() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::new(RouterBehavior::Success));
    let captured = router.captured.clone();
    let service = service(&pack, router);
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("Explain my Rust work.".to_owned(), sink)
        .await
        .expect("request starts");

    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::TextDelta {
            request_id,
            delta: "A streamed answer".to_owned(),
        }
    );
    assert!(matches!(
        next_event(&mut receiver).await,
        StreamEvent::Completed(CompletedResponse {
            request_id: completed_id,
            ..
        }) if completed_id == request_id
    ));

    let requests = captured.lock().expect("captured requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].user_text, "Explain my Rust work.");
    assert_eq!(requests[0].selected_context.documents.len(), 2);
    assert!(requests[0].system_prompt.contains("Uses Rust daily."));
}

#[tokio::test]
async fn prevents_duplicate_submissions_and_cancels_only_the_current_request() {
    let pack = PackFixture::new();
    let service = service(
        &pack,
        Arc::new(FakeRouter::new(RouterBehavior::WaitForCancellation)),
    );
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("First".to_owned(), sink.clone())
        .await
        .expect("first request starts");
    assert_eq!(
        service.start("Second".to_owned(), sink).await,
        Err(ManualAssistanceError::Busy)
    );
    assert_eq!(
        service.cancel(RequestId::new()),
        Err(ManualAssistanceError::NoMatchingRequest)
    );
    service.cancel(request_id).expect("current request cancels");

    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Started { request_id }
    );
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Cancelled { request_id }
    );
}

#[tokio::test]
async fn provider_failures_emit_safe_terminal_events_and_release_the_active_slot() {
    let pack = PackFixture::new();
    let service = service(
        &pack,
        Arc::new(FakeRouter::new(RouterBehavior::Failure(
            ProviderErrorKind::RateLimit,
        ))),
    );
    let (sink, mut receiver) = channel_sink();

    let request_id = service
        .start("First".to_owned(), sink.clone())
        .await
        .expect("request starts");
    let _ = next_event(&mut receiver).await;
    assert_eq!(
        next_event(&mut receiver).await,
        StreamEvent::Failed {
            request_id,
            error: ProviderError {
                kind: ProviderErrorKind::RateLimit,
                message: "Safe provider failure".to_owned(),
            },
        }
    );

    timeout(std::time::Duration::from_secs(1), async {
        loop {
            if !service.has_active_request() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("active slot released");
    service
        .start("Recovery".to_owned(), sink)
        .await
        .expect("new request starts after failure");
}
