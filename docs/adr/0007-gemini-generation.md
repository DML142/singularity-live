# Direct Gemini generation for text and screenshots

## Status

Accepted — 2026-09-27

## Context

The desktop client already supports manual text requests and explicit, transient screenshot
requests through a provider-neutral Rust port. The user has a Google AI Studio API key and
wants to use a capable text and image model without routing through OpenRouter. Voice
transcription remains a separate capability and is not provided by this generation key.

## Decision

- Add one direct Gemini adapter to the existing Rust text-generation port and route to it
  only when `SINGULARITY_LIVE_PROVIDER=gemini`.
- Use the Gemini `streamGenerateContent` SSE endpoint with a configured Gemini model ID,
  including text and request-scoped PNG image parts.
- Resolve `GEMINI_API_KEY` through the Rust `SecretStore`; send it only in the
  `x-goog-api-key` request header.
- Keep provider DTOs, endpoint construction, streaming parsing, status classification,
  cancellation, and timeout handling in the adapter. Do not retry or fall back to another
  provider.
- Let the interactive development launcher infer Gemini when the entered model ID begins
  with `gemini-`, then collect the key with hidden input. Do not persist the key.
- Keep audio capture, speech recognition, and voice-input controls outside this generation
  adapter.

## Alternatives considered

1. Continue using OpenRouter with the Google key. This key is not an OpenRouter credential
   and cannot authenticate to OpenRouter.
2. Put the Google key in React or a Tauri IPC request. That would expose a provider secret
   to the webview.
3. Add audio transcription in this change. Audio requires a separate capture and
   speech-to-text capability and is not part of the existing text-generation port.

## Consequences

Users can configure Gemini directly for text and reviewed screenshots while retaining the
existing OpenRouter option. Model availability and quota are determined by Google, and no
automatic fallback is attempted. `GEMINI_API_KEY` remains a development environment
credential; persistent OS-backed credential storage and Gemini live transcription remain
future work.
