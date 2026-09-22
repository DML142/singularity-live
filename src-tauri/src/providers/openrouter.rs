use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::{
        CompletedResponse, ProviderError, ProviderErrorKind, ProviderId, StreamEvent,
        TextGenerationRequest, Usage,
    },
    secrets::SecretValue,
};

use super::{StreamSink, TextGenerationProvider};

const OPENROUTER_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const MAX_STREAM_EVENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct OpenRouterAdapter {
    client: Client,
    endpoint: String,
    request_timeout: Duration,
}

impl OpenRouterAdapter {
    #[must_use]
    pub fn new(client: Client, request_timeout: Duration) -> Self {
        Self {
            client,
            endpoint: OPENROUTER_ENDPOINT.to_owned(),
            request_timeout,
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_endpoint(client: Client, endpoint: String, request_timeout: Duration) -> Self {
        Self {
            client,
            endpoint,
            request_timeout,
        }
    }

    async fn stream_inner(
        &self,
        request: &TextGenerationRequest,
        secret: &SecretValue,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        let payload = OpenRouterRequest::from(request);
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(secret.expose())
            .header("HTTP-Referer", "https://github.com/DML142/singularity-live")
            .header("X-OpenRouter-Title", "Singularity Live")
            .json(&payload)
            .send()
            .await
            .map_err(|error| classify_transport_error(&error))?;

        if !response.status().is_success() {
            return Err(classify_status(response.status()));
        }

        parse_stream(response, request, sink).await
    }
}

#[async_trait]
impl TextGenerationProvider for OpenRouterAdapter {
    async fn stream(
        &self,
        request: &TextGenerationRequest,
        secret: &SecretValue,
        cancellation: CancellationToken,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        tokio::select! {
            () = cancellation.cancelled() => Err(safe_error(
                ProviderErrorKind::Cancellation,
                "The request was cancelled",
            )),
            result = timeout(self.request_timeout, self.stream_inner(request, secret, sink)) => {
                result.unwrap_or_else(|_| Err(safe_error(
                    ProviderErrorKind::Timeout,
                    "The provider request timed out",
                )))
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenRouterRequest<'a> {
    model: &'a str,
    messages: [OpenRouterMessage<'a>; 2],
    stream: bool,
    stream_options: StreamOptions,
}

impl<'a> From<&'a TextGenerationRequest> for OpenRouterRequest<'a> {
    fn from(request: &'a TextGenerationRequest) -> Self {
        Self {
            model: request.model.as_str(),
            messages: [
                OpenRouterMessage {
                    role: "system",
                    content: &request.system_prompt,
                },
                OpenRouterMessage {
                    role: "user",
                    content: &request.user_text,
                },
            ],
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenRouterMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChunk {
    #[serde(default)]
    choices: Vec<OpenRouterChoice>,
    #[serde(default)]
    usage: Option<OpenRouterUsage>,
    #[serde(default)]
    error: Option<OpenRouterStreamError>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    delta: OpenRouterDelta,
}

#[derive(Debug, Deserialize)]
struct OpenRouterDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct OpenRouterUsage {
    #[serde(rename = "prompt_tokens")]
    prompt: u64,
    #[serde(rename = "completion_tokens")]
    completion: u64,
    #[serde(rename = "total_tokens")]
    total: u64,
}

impl From<OpenRouterUsage> for Usage {
    fn from(usage: OpenRouterUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.completion,
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenRouterStreamError {
    code: serde_json::Value,
}

async fn parse_stream(
    response: reqwest::Response,
    request: &TextGenerationRequest,
    sink: &dyn StreamSink,
) -> Result<CompletedResponse, ProviderError> {
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut usage = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| classify_transport_error(&error))?;
        buffer.extend_from_slice(&chunk);
        if buffer.len() > MAX_STREAM_EVENT_BYTES && find_event_end(&buffer).is_none() {
            return Err(malformed_response());
        }

        while let Some(event_end) = find_event_end(&buffer) {
            let delimiter_length = event_delimiter_length(&buffer, event_end);
            let event = buffer.drain(..event_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_length);
            match parse_event(&event, request, sink, &mut usage)? {
                EventState::Continue => {}
                EventState::Done => {
                    return Ok(CompletedResponse {
                        request_id: request.request_id,
                        provider: ProviderId::OpenRouter,
                        model: request.model.clone(),
                        usage,
                    });
                }
            }
        }
    }

    Err(malformed_response())
}

fn parse_event(
    event: &[u8],
    request: &TextGenerationRequest,
    sink: &dyn StreamSink,
    usage: &mut Option<Usage>,
) -> Result<EventState, ProviderError> {
    let event_text = std::str::from_utf8(event).map_err(|_| malformed_response())?;
    let data = event_text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(EventState::Continue);
    }
    if data == "[DONE]" {
        return Ok(EventState::Done);
    }

    let chunk = serde_json::from_str::<OpenRouterChunk>(&data).map_err(|_| malformed_response())?;
    if let Some(error) = chunk.error {
        return Err(classify_stream_error(&error));
    }
    if chunk.choices.is_empty() && chunk.usage.is_none() {
        return Err(malformed_response());
    }
    for choice in chunk.choices {
        if let Some(delta) = choice.delta.content.filter(|value| !value.is_empty()) {
            sink.emit(StreamEvent::TextDelta {
                request_id: request.request_id,
                delta,
            })?;
        }
    }
    if let Some(provider_usage) = chunk.usage {
        *usage = Some(provider_usage.into());
    }
    Ok(EventState::Continue)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventState {
    Continue,
    Done,
}

fn find_event_end(buffer: &[u8]) -> Option<usize> {
    let line_feed = buffer.windows(2).position(|window| window == b"\n\n");
    let carriage_return = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (line_feed, carriage_return) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    }
}

fn event_delimiter_length(buffer: &[u8], event_end: usize) -> usize {
    if buffer.get(event_end..event_end + 4) == Some(b"\r\n\r\n") {
        4
    } else {
        2
    }
}

fn classify_status(status: StatusCode) -> ProviderError {
    let kind = match status.as_u16() {
        401 | 403 => ProviderErrorKind::Authentication,
        400 | 404 | 422 => ProviderErrorKind::InvalidRequest,
        408 | 504 => ProviderErrorKind::Timeout,
        429 => ProviderErrorKind::RateLimit,
        _ => ProviderErrorKind::Provider,
    };
    safe_error(kind, "The provider rejected the request")
}

fn classify_stream_error(error: &OpenRouterStreamError) -> ProviderError {
    let kind = if error.code.as_u64() == Some(429) {
        ProviderErrorKind::RateLimit
    } else {
        ProviderErrorKind::Provider
    };
    safe_error(kind, "The provider stopped the response")
}

fn classify_transport_error(error: &reqwest::Error) -> ProviderError {
    let kind = if error.is_timeout() {
        ProviderErrorKind::Timeout
    } else {
        ProviderErrorKind::Transport
    };
    safe_error(kind, "The provider could not be reached")
}

fn malformed_response() -> ProviderError {
    safe_error(
        ProviderErrorKind::MalformedResponse,
        "The provider returned an invalid streaming response",
    )
}

fn safe_error(kind: ProviderErrorKind, message: &str) -> ProviderError {
    ProviderError {
        kind,
        message: message.to_owned(),
    }
}
