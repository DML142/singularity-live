use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use singularity_live::{
    domain::{
        ModelId, ProviderError, ProviderErrorKind, ProviderId, RequestId, SelectedContext,
        StreamEvent, TextGenerationRequest, Usage,
    },
    providers::{OpenRouterAdapter, StreamSink, TextGenerationProvider},
    secrets::SecretValue,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<StreamEvent>>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<StreamEvent> {
        self.events.lock().expect("event lock").clone()
    }
}

impl StreamSink for RecordingSink {
    fn emit(&self, event: StreamEvent) -> Result<(), ProviderError> {
        self.events.lock().expect("event lock").push(event);
        Ok(())
    }
}

struct MockResponse {
    status: u16,
    body_chunks: Vec<&'static str>,
    delay_before_response: Duration,
}

async fn start_server(response: MockResponse) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local server");
    let address = listener.local_addr().expect("local address");
    let (request_sender, request_receiver) = oneshot::channel();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept request");
        let request = read_http_request(&mut socket).await;
        let _ = request_sender.send(request);
        sleep(response.delay_before_response).await;

        let body_length = response
            .body_chunks
            .iter()
            .map(|chunk| chunk.len())
            .sum::<usize>();
        let reason = if response.status == 200 {
            "OK"
        } else {
            "Error"
        };
        let headers = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: text/event-stream\r\nContent-Length: {body_length}\r\nConnection: close\r\n\r\n",
            response.status
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("write response headers");
        for chunk in response.body_chunks {
            socket
                .write_all(chunk.as_bytes())
                .await
                .expect("write response chunk");
            socket.flush().await.expect("flush response chunk");
        }
    });

    (
        format!("http://{address}/api/v1/chat/completions"),
        request_receiver,
    )
}

async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let count = socket.read(&mut buffer).await.expect("read request");
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or_default();
        if bytes.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8(bytes).expect("request is UTF-8")
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn request() -> TextGenerationRequest {
    TextGenerationRequest {
        request_id: RequestId::new(),
        provider: ProviderId::OpenRouter,
        model: ModelId::new("openrouter/free").expect("model"),
        selected_context: SelectedContext::default(),
        system_prompt: "Use only relevant context.".to_owned(),
        user_text: "Explain ownership.".to_owned(),
    }
}

fn secret() -> SecretValue {
    SecretValue::new("sensitive-test-value".to_owned()).expect("test credential")
}

#[tokio::test]
async fn translates_requests_and_streams_text_with_usage() {
    let (endpoint, captured_request) = start_server(MockResponse {
        status: 200,
        body_chunks: vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Own\"}}]}\n",
            "\ndata: {\"choices\":[{\"delta\":{\"content\":\"ership\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":2,\"total_tokens\":14}}\n\n",
            "data: [DONE]\n\n",
        ],
        delay_before_response: Duration::ZERO,
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_secs(2),
    );
    let sink = RecordingSink::default();
    let generation_request = request();

    let completed = adapter
        .stream(
            &generation_request,
            &secret(),
            CancellationToken::new(),
            &sink,
        )
        .await
        .expect("stream completes");

    let raw_request = captured_request.await.expect("captured request");
    assert!(raw_request.starts_with("POST /api/v1/chat/completions HTTP/1.1"));
    assert!(raw_request.contains("authorization: Bearer sensitive-test-value"));
    let body = raw_request.split("\r\n\r\n").nth(1).expect("request body");
    let json: serde_json::Value = serde_json::from_str(body).expect("request JSON");
    assert_eq!(json["model"], "openrouter/free");
    assert_eq!(json["stream"], true);
    assert_eq!(json["stream_options"]["include_usage"], true);
    assert_eq!(json["messages"][0]["role"], "system");
    assert_eq!(
        json["messages"][0]["content"],
        generation_request.system_prompt
    );
    assert_eq!(json["messages"][1]["role"], "user");
    assert_eq!(json["messages"][1]["content"], generation_request.user_text);
    assert_eq!(
        sink.events(),
        vec![
            StreamEvent::TextDelta {
                request_id: generation_request.request_id,
                delta: "Own".to_owned(),
            },
            StreamEvent::TextDelta {
                request_id: generation_request.request_id,
                delta: "ership".to_owned(),
            },
        ]
    );
    assert_eq!(
        completed.usage,
        Some(Usage {
            input_tokens: 12,
            output_tokens: 2,
            total_tokens: 14,
        })
    );
}

#[tokio::test]
async fn rejects_malformed_and_incomplete_streams() {
    for body in [
        "data: not-json\n\n",
        "data: {}\n\n",
        "data: {\"choices\":[]}\n\n",
    ] {
        let (endpoint, _) = start_server(MockResponse {
            status: 200,
            body_chunks: vec![body],
            delay_before_response: Duration::ZERO,
        })
        .await;
        let adapter = OpenRouterAdapter::with_test_endpoint(
            reqwest::Client::new(),
            endpoint,
            Duration::from_secs(2),
        );

        let error = adapter
            .stream(
                &request(),
                &secret(),
                CancellationToken::new(),
                &RecordingSink::default(),
            )
            .await
            .expect_err("malformed stream must fail");

        assert_eq!(error.kind, ProviderErrorKind::MalformedResponse);
    }
}

#[tokio::test]
async fn rejects_a_complete_stream_event_over_the_size_limit() {
    let content = "x".repeat(70 * 1024);
    let event = Box::leak(
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{content}\"}}}}]}}\n\n")
            .into_boxed_str(),
    );
    let (endpoint, _) = start_server(MockResponse {
        status: 200,
        body_chunks: vec![event],
        delay_before_response: Duration::ZERO,
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_secs(2),
    );
    let sink = RecordingSink::default();

    let error = adapter
        .stream(&request(), &secret(), CancellationToken::new(), &sink)
        .await
        .expect_err("oversized event must fail");

    assert_eq!(error.kind, ProviderErrorKind::MalformedResponse);
    assert!(sink.events().is_empty());
}

#[tokio::test]
async fn classifies_http_failures_without_returning_provider_bodies() {
    for (status, expected_kind) in [
        (401, ProviderErrorKind::Authentication),
        (403, ProviderErrorKind::Authentication),
        (400, ProviderErrorKind::InvalidRequest),
        (404, ProviderErrorKind::InvalidRequest),
        (422, ProviderErrorKind::InvalidRequest),
        (429, ProviderErrorKind::RateLimit),
        (408, ProviderErrorKind::Timeout),
        (504, ProviderErrorKind::Timeout),
        (500, ProviderErrorKind::Provider),
    ] {
        let (endpoint, _) = start_server(MockResponse {
            status,
            body_chunks: vec!["sensitive provider detail"],
            delay_before_response: Duration::ZERO,
        })
        .await;
        let adapter = OpenRouterAdapter::with_test_endpoint(
            reqwest::Client::new(),
            endpoint,
            Duration::from_secs(2),
        );

        let error = adapter
            .stream(
                &request(),
                &secret(),
                CancellationToken::new(),
                &RecordingSink::default(),
            )
            .await
            .expect_err("HTTP failure must be classified");

        assert_eq!(error.kind, expected_kind, "status {status}");
        assert!(!error.to_string().contains("sensitive provider detail"));
    }
}

#[tokio::test]
async fn classifies_provider_error_events() {
    let (endpoint, _) = start_server(MockResponse {
        status: 200,
        body_chunks: vec!["data: {\"error\":{\"code\":429,\"message\":\"private detail\"}}\n\n"],
        delay_before_response: Duration::ZERO,
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_secs(2),
    );

    let error = adapter
        .stream(
            &request(),
            &secret(),
            CancellationToken::new(),
            &RecordingSink::default(),
        )
        .await
        .expect_err("provider error event must fail");

    assert_eq!(error.kind, ProviderErrorKind::RateLimit);
    assert!(!error.to_string().contains("private detail"));
}

#[tokio::test]
async fn classifies_connection_failures_as_transport_errors() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve local address");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        format!("http://{address}/api/v1/chat/completions"),
        Duration::from_secs(2),
    );

    let error = adapter
        .stream(
            &request(),
            &secret(),
            CancellationToken::new(),
            &RecordingSink::default(),
        )
        .await
        .expect_err("connection failure must be classified");

    assert_eq!(error.kind, ProviderErrorKind::Transport);
}

#[tokio::test]
async fn enforces_provider_stream_timeout() {
    let (endpoint, _) = start_server(MockResponse {
        status: 200,
        body_chunks: vec!["data: [DONE]\n\n"],
        delay_before_response: Duration::from_millis(200),
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_millis(20),
    );

    let error = adapter
        .stream(
            &request(),
            &secret(),
            CancellationToken::new(),
            &RecordingSink::default(),
        )
        .await
        .expect_err("slow response must time out");

    assert_eq!(error.kind, ProviderErrorKind::Timeout);
}

#[tokio::test]
async fn cancels_while_waiting_for_a_response() {
    let (endpoint, _) = start_server(MockResponse {
        status: 200,
        body_chunks: vec!["data: [DONE]\n\n"],
        delay_before_response: Duration::from_secs(1),
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_secs(2),
    );
    let cancellation = CancellationToken::new();
    let cancellation_trigger = cancellation.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        cancellation_trigger.cancel();
    });

    let error = adapter
        .stream(
            &request(),
            &secret(),
            cancellation,
            &RecordingSink::default(),
        )
        .await
        .expect_err("cancelled request must stop");

    assert_eq!(error.kind, ProviderErrorKind::Cancellation);
}
