use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    domain::{
        CompletedResponse, ConversationRole, MessagePart, ProviderError, ProviderErrorKind,
        ProviderId, StreamEvent, TextGenerationRequest, Usage,
    },
    secrets::SecretValue,
};

use super::{StreamSink, TextGenerationProvider};

const GEMINI_ENDPOINT_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/";
const MAX_STREAM_EVENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct GeminiAdapter {
    client: Client,
    request_timeout: Duration,
}

impl GeminiAdapter {
    #[must_use]
    pub fn new(client: Client, request_timeout: Duration) -> Self {
        Self {
            client,
            request_timeout,
        }
    }

    async fn stream_inner(
        &self,
        request: &TextGenerationRequest,
        secret: &SecretValue,
        sink: &dyn StreamSink,
    ) -> Result<CompletedResponse, ProviderError> {
        let endpoint = endpoint_for_model(request.model.as_str())?;
        let payload = GeminiRequest::from(request);
        let response = self
            .client
            .post(endpoint)
            .header("x-goog-api-key", secret.expose())
            .header("Accept", "text/event-stream")
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
impl TextGenerationProvider for GeminiAdapter {
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    system_instruction: GeminiContent,
    contents: Vec<GeminiMessage>,
}

impl From<&TextGenerationRequest> for GeminiRequest {
    fn from(request: &TextGenerationRequest) -> Self {
        let contents = request
            .messages
            .iter()
            .map(|message| GeminiMessage {
                role: match message.role {
                    ConversationRole::User => "user",
                    ConversationRole::Assistant => "model",
                },
                parts: message
                    .parts
                    .iter()
                    .map(|part| match part {
                        MessagePart::Text(text) => json!({"text": text}),
                        MessagePart::Image(image) => json!({
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": STANDARD.encode(image.png_bytes()),
                            }
                        }),
                    })
                    .collect(),
            })
            .collect();

        Self {
            system_instruction: GeminiContent {
                parts: vec![json!({"text": request.system_prompt})],
            },
            contents,
        }
    }
}

#[derive(serde::Serialize)]
struct GeminiMessage {
    role: &'static str,
    parts: Vec<Value>,
}

#[derive(serde::Serialize)]
struct GeminiContent {
    parts: Vec<Value>,
}

fn endpoint_for_model(model: &str) -> Result<Url, ProviderError> {
    let mut endpoint = Url::parse(GEMINI_ENDPOINT_ROOT).map_err(|_| {
        safe_error(
            ProviderErrorKind::Configuration,
            "Gemini endpoint configuration is invalid",
        )
    })?;
    let operation = format!("{model}:streamGenerateContent");
    endpoint
        .path_segments_mut()
        .map_err(|()| {
            safe_error(
                ProviderErrorKind::Configuration,
                "Gemini endpoint configuration is invalid",
            )
        })?
        .pop_if_empty()
        .push("models")
        .push(&operation);
    endpoint.query_pairs_mut().append_pair("alt", "sse");
    Ok(endpoint)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiChunk {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    usage_metadata: Option<GeminiUsage>,
    prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt: u64,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates: u64,
    #[serde(rename = "totalTokenCount", default)]
    total: u64,
}

impl From<GeminiUsage> for Usage {
    fn from(usage: GeminiUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.candidates,
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPromptFeedback {
    block_reason: Option<String>,
}

async fn parse_stream(
    response: reqwest::Response,
    request: &TextGenerationRequest,
    sink: &dyn StreamSink,
) -> Result<CompletedResponse, ProviderError> {
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut usage = None;
    let mut saw_data = false;
    let mut finished = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| classify_transport_error(&error))?;
        buffer.extend_from_slice(&chunk);
        if buffer.len() > MAX_STREAM_EVENT_BYTES && find_event_end(&buffer).is_none() {
            return Err(malformed_response());
        }

        while let Some(event_end) = find_event_end(&buffer) {
            if event_end > MAX_STREAM_EVENT_BYTES {
                return Err(malformed_response());
            }
            let delimiter_length = event_delimiter_length(&buffer, event_end);
            let event = buffer.drain(..event_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_length);
            if parse_event(
                &event,
                request,
                sink,
                &mut usage,
                &mut saw_data,
                &mut finished,
            )? == EventState::Done
            {
                return Ok(completed_response(request, usage));
            }
        }
    }

    if !buffer.is_empty() {
        parse_event(
            &buffer,
            request,
            sink,
            &mut usage,
            &mut saw_data,
            &mut finished,
        )?;
    }
    if !saw_data || !finished {
        return Err(malformed_response());
    }
    Ok(completed_response(request, usage))
}

fn parse_event(
    event: &[u8],
    request: &TextGenerationRequest,
    sink: &dyn StreamSink,
    usage: &mut Option<Usage>,
    saw_data: &mut bool,
    finished: &mut bool,
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
    *saw_data = true;
    if data == "[DONE]" {
        *finished = true;
        return Ok(EventState::Done);
    }

    let chunk = serde_json::from_str::<GeminiChunk>(&data).map_err(|_| malformed_response())?;
    if chunk
        .prompt_feedback
        .and_then(|feedback| feedback.block_reason)
        .is_some()
    {
        return Err(safe_error(
            ProviderErrorKind::InvalidRequest,
            "Google blocked this request; adjust the prompt or screenshot and try again",
        ));
    }
    if chunk.candidates.is_empty() && chunk.usage_metadata.is_none() {
        return Err(malformed_response());
    }
    for candidate in chunk.candidates {
        if candidate.finish_reason.is_some() {
            *finished = true;
        }
        if let Some(content) = candidate.content {
            for part in content.parts {
                if !part.thought
                    && let Some(text) = part.text.filter(|text| !text.is_empty())
                {
                    sink.emit(StreamEvent::TextDelta {
                        request_id: request.request_id,
                        delta: text,
                    })?;
                }
            }
        }
    }
    if let Some(provider_usage) = chunk.usage_metadata {
        *usage = Some(provider_usage.into());
    }
    Ok(EventState::Continue)
}

fn completed_response(request: &TextGenerationRequest, usage: Option<Usage>) -> CompletedResponse {
    CompletedResponse {
        request_id: request.request_id,
        provider: ProviderId::Gemini,
        model: request.model.clone(),
        usage,
    }
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
    let (kind, message) = match status.as_u16() {
        401 | 403 => (
            ProviderErrorKind::Authentication,
            "Google rejected the API key or denied Gemini API access",
        ),
        400 | 404 | 422 => (
            ProviderErrorKind::InvalidRequest,
            "Google rejected the Gemini model or request format",
        ),
        408 | 504 => (ProviderErrorKind::Timeout, "The Gemini request timed out"),
        429 => (
            ProviderErrorKind::RateLimit,
            "Gemini API rate limit reached",
        ),
        _ => (
            ProviderErrorKind::Provider,
            "Google rejected the Gemini request",
        ),
    };
    safe_error(kind, &format!("{message} (HTTP {})", status.as_u16()))
}

fn classify_transport_error(error: &reqwest::Error) -> ProviderError {
    let kind = if error.is_timeout() {
        ProviderErrorKind::Timeout
    } else {
        ProviderErrorKind::Transport
    };
    safe_error(kind, "The Gemini API could not be reached")
}

fn malformed_response() -> ProviderError {
    safe_error(
        ProviderErrorKind::MalformedResponse,
        "Gemini returned an invalid streaming response",
    )
}

fn safe_error(kind: ProviderErrorKind, message: &str) -> ProviderError {
    ProviderError {
        kind,
        message: message.to_owned(),
    }
}
