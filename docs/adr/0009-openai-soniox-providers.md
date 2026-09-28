# OpenAI generation and Soniox voice transcription

## Status

Accepted — 2026-09-28

## Context

The creator workflow needs strong text and image understanding for code and explanations,
alongside low-latency incoming speech transcription. These capabilities have different model
requirements: GPT-6 Luna generates text from text and screenshots, while Soniox streams audio
transcription. Assistant replies are text only.

## Decision

- Select OpenAI `gpt-6-luna` as the default text and screenshot model through the existing
  provider-neutral Rust generation port. Keep Gemini and OpenRouter as optional alternatives.
- Use OpenAI Chat Completions streaming for text and request-scoped PNG images. Keep provider
  DTOs, endpoint construction, authorization, SSE parsing, status mapping, timeout, and
  cancellation inside the Rust adapter.
- Use Soniox `stt-rt-v5` over its WebSocket API for incoming speech-to-text. Send Russian,
  Ukrainian, and English language hints plus common programming terms. Do not request spoken
  assistant output.
- Keep `OPENAI_API_KEY` and `SONIOX_API_KEY` in the desktop process environment. The
  development launcher accepts them through hidden input and passes them only to the child
  process. React and Tauri IPC never receive provider keys.
- Capture microphone or Windows system-audio loopback in Rust through WASAPI. A local RMS
  energy gate keeps a short pre-roll and speech tail and discards sustained silence before
  audio leaves the machine.
- Show live transcription in the transcript panel. On stop, insert the finalized text into
  the editable composer. Text generation remains a separate explicit user action.
- Discard raw audio after streaming it. Do not persist audio or transcripts as session
  history.

## Alternatives considered

1. Use one multimodal model for text, images, and speech. The selected generation model does
   not accept audio, and coupling transcription to generation would prevent independently
   selecting a fast streaming STT provider.
2. Send audio from React directly to Soniox. That would expose the provider credential to the
   webview and move OS capture orchestration outside the Rust boundary.
3. Use Gemini Live for transcription. The user selected Soniox for its streaming workflow and
   low expected cost; Gemini remains available for text/image generation only.

## Consequences

The app now has independent generation and transcription providers with distinct keys and
typed Rust boundaries. The initial capture adapter supports microphone and default render
loopback on Windows; other platforms report voice capture as unavailable. Silence is gated by
local RMS energy, which may need threshold tuning for quiet microphones or noisy environments.
The app has not yet been verified with live provider keys or real audio devices. Reconnection
and measured latency remain follow-up work.
