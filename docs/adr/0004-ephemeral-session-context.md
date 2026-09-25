# Ephemeral session context and Rust-owned lifecycle

## Status

Accepted — 2026-09-25

## Context

The manual-assistance view can show several user and assistant turns, but presentation-only
history does not give follow-up requests real conversation context. The application needs
one active session that can carry completed turns and automatically selected supported
context into the next provider request. Context must remain bounded, and the existing
provider boundary and cancellation behavior must remain in Rust. Persistent history is
reserved for the later storage stage.

## Decision

- A Rust `SessionService` owns one process-local session, its lifecycle, completed turn
  history, rolling summary, request reservation, and cancellation token.
- Requests include completed user and assistant messages in chronological role-tagged order,
  followed by the current user message. Only successfully completed exchanges enter model
  context; failed and cancelled partial answers remain presentation-only.
- Static context selection stays deterministic: include `always_include` documents and
  keyword matches for the current request in manifest order. Rust builds a bounded system
  prompt with summary and selected context.
- Keep at most 8 recent exchanges and 16 KiB of recent history, 20 KiB of system context,
  4 KiB of rolling summary, 16 KiB of each retained assistant answer, and 64 KiB of combined
  text for a summary request. All limits count UTF-8 bytes.
- When raw history exceeds either recent-history limit, compact the oldest complete turns
  through the existing configured Rust text-generation router. Stage all summary batches
  and commit them only after they succeed. No summary provider call is made within the
  recent-history bounds.
- A narrow `reset_session` command clears the summary and turns when no request is active.
  React exposes **New session** and clears its visible turns only after that command
  succeeds.
- Session state is volatile and disappears on reset or process restart. This decision adds
  no SQLite, multi-chat support, screenshot or image input, audio, transcription, or response
  modes.

## Alternatives considered

1. Keep the recent transcript only in React. The provider would still receive no genuine
   follow-up context, and the webview would own model-context selection.
2. Send all session turns without bounds. Context growth would be unbounded, and older
   relevant intent would compete with recent requests.
3. Persist turns and summaries immediately. That couples this feature to Phase 5 storage
   and retention policy, which is not needed for one active in-memory session.

## Consequences

Follow-up requests can retain prior intent, including a request that is waiting for a
supported text artifact. Long sessions make additional model calls to summarize old turns,
so compaction adds provider latency and can fail safely without losing old context. Session
data is intentionally lost at reset and application restart. Static document relevance is
still limited to the existing keyword selector; richer intent routing and multimodal inputs
remain later work.
