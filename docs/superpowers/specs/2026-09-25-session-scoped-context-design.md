# Session-Scoped Context Design

**Status:** Approved for implementation planning  
**Date:** 2026-09-25

## Purpose

Make follow-up requests in the single active session use genuine conversation context.
The Rust application service owns recent turns, context selection, rolling summaries,
session lifecycle, and cancellation. React presents the conversation and sends the new
user input; it does not choose or assemble model context.

For example, after a user asks what a code sample does and the assistant asks to see the
code, a later supported input containing that code must retain the earlier request so the
model knows to explain it. The current repository supports manual text only. This stage
does not add screenshot capture, image input, audio, transcription, or other modalities.
When image input is implemented in its planned stage, it should enter the same Rust-owned
session and combine with the prior user intent.

## Scope

### Included

- One process-local, in-memory session with an explicit reset action.
- Recent successful user/assistant exchanges passed to the provider as role-tagged messages.
- Automatic static context selection using the existing deterministic manifest-order,
  `always_include`, and keyword rules.
- Bounded request context and provider-generated rolling summaries for older exchanges.
- Rust-owned request lifecycle and cancellation across summarization and answer generation.
- Safe UI states for reset, cancellation, and recoverable request errors.
- Mock-provider tests and updates to `tech.md` and the ADR set.

### Excluded

- Multiple concurrent or selectable chats, durable chat history, SQLite, or Phase 5 work.
- Screenshot capture, image processing, multimodal provider requests, audio, transcription,
  or new response modes.
- Live OpenRouter calls in routine tests.

## Architecture

### Session owner

A Rust `SessionService` is the sole owner of one in-memory session. It owns the configured
context-pack location and text-generation router, the session state, and the active request
cancellation token. The session starts in `Idle`; the first successful exchange moves it to
`Active`. A request moves it to `Processing`. Completion returns it to `Active`; failure or
cancellation returns it to the prior usable state (`Idle` if no exchange has completed,
otherwise `Active`).

The session state contains a generated session identifier, lifecycle state, an optional
rolling summary, and bounded completed exchanges. No session data is written to disk. A
process restart creates an empty session. Reset clears the summary and exchanges and returns
the service to `Idle` with a fresh session identifier.

### Request construction

For a manual-text request, Rust loads and validates the configured context pack on a
blocking worker, selects documents against the current user text, and builds a
provider-neutral generation request. The system message contains the safety instructions,
selected static documents, and rolling summary. It is followed by completed prior user and
assistant messages in chronological order and the current user message. The OpenRouter
adapter maps those typed roles to its chat-completions request format. Provider-specific
request details remain inside that adapter.

Rust wraps the existing stream sink so response deltas continue to reach the UI while the
completed answer is collected for session state. Only a successful terminal response
commits the current user/assistant exchange. A cancelled or failed partial turn remains in
the frontend's visible transcript but is excluded from future model context.

### Automatic context selection

The user does not choose history or context documents manually. Recent conversation is
included automatically according to the bounds below. Static context keeps the current
deterministic selector: include documents marked `always_include` and documents whose
configured keywords match the current user text, in manifest order. Unmatched documents
remain excluded. The model receives prior intent as role-tagged conversation history.

This stage handles text requests only. A later image-capable path must supply its image to
the same session request while retaining the preceding textual intent; this document does
not claim image support now.

## Context bounds and rolling summaries

All limits count UTF-8 content bytes and require no tokenizer dependency:

| Context layer | Limit |
| --- | ---: |
| Current manual input | 16 KiB (existing limit) |
| Recent completed exchanges | 8 exchanges and 16 KiB total |
| System context, including instructions, static documents, and summary | 20 KiB |
| Rolling summary | 4 KiB |
| Combined system and conversation text | 64 KiB maximum |
| Assistant response retained for model context per exchange | 16 KiB |

The independent layer limits leave room below the combined cap. The UI continues to display
the complete streamed answer; only the session copy used for future model context is capped
at 16 KiB. UTF-8 truncation must stop at a character boundary.

The system-context budget reserves room for instructions and summary first, then adds
selected document content in manifest order. If the next document exceeds the remaining
budget, its content is truncated at a character boundary and later documents are omitted.

When completed exchanges exceed either recent-history limit, Rust stages a compaction of
the oldest complete exchanges until the remaining raw history is at most 8 exchanges and
16 KiB. Each summary request includes the current summary and the largest oldest prefix that
fits within a 64 KiB summary-request text budget. A summary request can contain multiple old
exchanges; repeated requests are made only if needed to satisfy both raw-history limits.
The summary instruction preserves user goals, constraints, unresolved questions, and
pending requests such as waiting for an artifact. Each result is capped at 4 KiB. Staged
summaries and removed turns replace session state only after compaction completes
successfully; a failed or cancelled batch leaves the prior summary and turns unchanged.

Summarization is a provider call through the existing Rust router and runs only when the
recent-history bound is exceeded. If it fails, returns no usable text, or is cancelled,
the prior summary and turns remain unchanged and the request ends in a safe recoverable
error or cancellation state. A successful summary may remain committed if the subsequent
answer generation fails; it represents only previously completed exchanges.

## Lifecycle, cancellation, and errors

- `reset_session` is a narrow typed Rust command. The UI exposes it as **New session**.
- Reset is rejected with `Busy` while a request is processing. The UI disables the action
  during processing and clears its visible turns only after Rust confirms reset success.
- The active request's cancellation token covers context preparation, summary generation,
  and answer generation. Cancellation emits the existing typed cancelled event.
- Context loading, summary generation, and provider failures use safe errors; sensitive
  text, selected context, prompts, and credentials are never logged.
- A failed request does not fail the entire session. Previously completed context remains
  available for retry.

## Validation

Automated tests use a mock `TextGenerationRouter`; they do not require credentials or make
network calls. Focused backend coverage must verify:

1. A follow-up request carries prior completed user and assistant turns in chronological
   role-tagged order.
2. Recent turns and combined request context remain within their byte and exchange limits.
3. Exceeding the recent bound invokes summarization, preserves a rolling summary and newest
   turns, and keeps pending user intent in the summary instruction.
4. Failed, empty, and cancelled summaries do not discard prior context.
5. Static documents matching the current request are selected while unrelated documents
   are excluded, except for explicit `always_include` documents.
6. Reset clears recent turns and summary so the next request contains no prior-session data.
7. Cancellation during summary and answer generation emits the correct terminal state and
   does not commit a partial exchange.
8. Provider/context errors produce safe UI-visible failures and do not corrupt the session.

Frontend tests must verify that reset calls the Rust command, clears displayed turns only
after success, is unavailable while processing, and leaves the existing conversation intact
on reset failure. Run the project's full `pnpm check` before claiming acceptance.

## Architecture record

Add ADR 0004 to record the single volatile Rust-owned session, provider-mediated rolling
summary, and the no-persistence boundary for this stage. Update `tech.md` to distinguish the
implemented text session from remaining multimodal Phase 4 scope. Phase 4 remains in
progress until its full acceptance criteria are met.
