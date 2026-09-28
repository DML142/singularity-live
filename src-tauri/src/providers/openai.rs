use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
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

const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
const MAX_STREAM_EVENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct OpenAiAdapter {
    client: Client,
    endpoint: String,
    request_timeout: Duration,
}

impl OpenAiAdapter {
    #[must_use]
    pub fn new(client: Client, request_timeout: Duration) -> Self {
        Self {
            client,
            endpoint: OPENAI_ENDPOINT.to_owned(),
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
        let payload = OpenAIRequest::from(request);
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(secret.expose())
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
impl TextGenerationProvider for OpenAiAdapter {
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

#[derive(Serialize)]
struct OpenAIRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAIMessage>,
    stream: bool,
    stream_options: StreamOptions,
}

impl<'a> From<&'a TextGenerationRequest> for OpenAIRequest<'a> {
    fn from(request: &'a TextGenerationRequest) -> Self {
        let mut messages = Vec::with_capacity(request.messages.len() + 1);
        messages.push(OpenAIMessage {
            role: "system",
            content: serde_json::Value::String(request.system_prompt.clone()),
        });
        messages.extend(request.messages.iter().map(|message| {
            let mut image_parts = Vec::new();
            let mut text_content = None;
            for part in &message.parts {
                match part {
                    MessagePart::Text(text) => {
                        text_content = Some(text.as_str());
                        image_parts.push(serde_json::json!({"type": "text", "text": text}));
                    }
                    MessagePart::Image(image) => {
                        let data_url = format!(
                            "data:image/png;base64,{}",
                            STANDARD.encode(image.png_bytes())
                        );
                        image_parts.push(serde_json::json!({
                            "type": "image_url",
                            "image_url": {"url": data_url}
                        }));
                    }
                }
            }
            let content = if image_parts.iter().any(|part| part["type"] == "image_url") {
                serde_json::Value::Array(image_parts)
            } else {
                serde_json::Value::String(text_content.unwrap_or_default().to_owned())
            };
            OpenAIMessage {
                role: match message.role {
                    ConversationRole::User => "user",
                    ConversationRole::Assistant => "assistant",
                },
                content,
            }
        }));

        Self {
            model: request.model.as_str(),
            messages,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        }
    }
}

#[derive(Serialize)]
struct OpenAIMessage {
    role: &'static str,
    content: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAIChunk {
    #[serde(default)]
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
    #[serde(default)]
    error: Option<OpenAIStreamError>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    delta: OpenAIDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAIDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct OpenAIUsage {
    #[serde(rename = "prompt_tokens")]
    prompt: u64,
    #[serde(rename = "completion_tokens")]
    completion: u64,
    #[serde(rename = "total_tokens")]
    total: u64,
}

impl From<OpenAIUsage> for Usage {
    fn from(usage: OpenAIUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.completion,
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamError {
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
            if event_end > MAX_STREAM_EVENT_BYTES {
                return Err(malformed_response());
            }
            let delimiter_length = event_delimiter_length(&buffer, event_end);
            let event = buffer.drain(..event_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_length);
            match parse_event(&event, request, sink, &mut usage)? {
                EventState::Continue => {}
                EventState::Done => {
                    return Ok(CompletedResponse {
                        request_id: request.request_id,
                        provider: ProviderId::OpenAi,
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

    let chunk = serde_json::from_str::<OpenAIChunk>(&data).map_err(|_| malformed_response())?;
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
    let (kind, message) = match status.as_u16() {
        401 => (
            ProviderErrorKind::Authentication,
            "OpenAI rejected the API key",
        ),
        403 => (
            ProviderErrorKind::Authentication,
            "OpenAI denied access to this account or model",
        ),
        402 => (
            ProviderErrorKind::Provider,
            "OpenAI requires credits or an available free-request quota",
        ),
        400 | 422 => (
            ProviderErrorKind::InvalidRequest,
            "OpenAI rejected the model or request format",
        ),
        404 => (
            ProviderErrorKind::InvalidRequest,
            "OpenAI could not find an available endpoint for this model",
        ),
        408 | 504 => (ProviderErrorKind::Timeout, "OpenAI request timed out"),
        429 => (ProviderErrorKind::RateLimit, "OpenAI rate limit reached"),
        _ => (ProviderErrorKind::Provider, "OpenAI rejected the request"),
    };
    safe_error(kind, &format!("{message} (HTTP {})", status.as_u16()))
}

fn classify_stream_error(error: &OpenAIStreamError) -> ProviderError {
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

#[cfg(test)]
mod tests {
    use super::OpenAIRequest;
    use crate::domain::{
        ConversationMessage, ModelId, ProviderId, RequestId, SelectedContext, TextGenerationRequest,
    };

    #[test]
    fn maps_text_and_request_scoped_images_to_chat_completions() {
        let request = TextGenerationRequest {
            request_id: RequestId::new(),
            provider: ProviderId::OpenAi,
            model: ModelId::new("gpt-6-luna").expect("model ID"),
            selected_context: SelectedContext::default(),
            system_prompt: "Answer clearly".to_owned(),
            messages: vec![ConversationMessage::user_with_png(
                "Explain this screen",
                vec![1, 2, 3],
            )],
        };

        let value = serde_json::to_value(OpenAIRequest::from(&request)).expect("request JSON");

        assert_eq!(value["model"], "gpt-6-luna");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][0]["content"], "Answer clearly");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(
            value["messages"][1]["content"][0]["text"],
            "Explain this screen"
        );
        assert_eq!(
            value["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
    }
}
