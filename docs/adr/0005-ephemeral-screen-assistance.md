# Ephemeral screen assistance and provider-neutral image requests

## Status

Accepted — 2026-09-26

## Context

The manual text session can preserve a user's question and answer, but users also need a
deliberate way to show the assistant a screen, window, or relevant region. Screen content is
sensitive and must not be captured in the background, retained with chat history, or written
to logs or disk. Capture permissions, platform behavior, image processing, session context,
provider calls, cancellation, and deletion must stay inside the Rust boundary.

## Decision

- Expose narrow commands for capability reporting, target listing, explicit capture,
  cancellation, in-memory crop, discard, and sending one screenshot with prior conversation
  context. The Tauri main-window capability grants only these commands.
- Use XCap for one-shot monitor/window frames on Windows, macOS, and X11. On Linux Wayland,
  use the visible XDG ScreenCast source picker, `PersistMode::DoNot`, and one PipeWire frame;
  do not use XCap's temporary-file Wayland path.
- Normalize screenshots in memory to bounded PNG data. Keep a single opaque-ID image in a
  Rust-owned in-memory store for at most five minutes. Delete it on replacement, expiry,
  send, discard, reset, failure, cancellation, and process exit. The UI may retain a base64
  preview in the current conversation until the image's original expiry, reset, or unmount.
- Let the user review the preview in chat, add a text note, choose one short context action,
  and explicitly confirm sending it. Region selection is a validated crop of that in-memory
  preview. Unsupported source types and permission errors are reported through safe capability
  and error states.
- Represent images as request-scoped, provider-neutral message parts. A screenshot request
  uses its current note to select static context and sends prior role-tagged text turns with
  the current user text and image. A previous text request is optional. Completed session
  history retains text only; image bytes never enter Rust history or summaries.
- Map image parts to OpenRouter's image URL content part using an in-memory base64 data URL.
  Preserve the existing text-only request wire shape. Use local mock providers in automated
  tests; do not make live provider requests as part of routine validation.
- Do not add OCR, audio, transcription, persistent media, SQLite, or automatic/background
  capture.

## Alternatives considered

1. Capture the desktop continuously and attach the latest frame. This would capture without
   a specific user action and increase the risk of unrelated sensitive content entering a
   provider request.
2. Persist screenshots alongside text turns. This would expand retention and require the
   later storage and attachment lifecycle; the current feature only needs one active request.
3. Use XCap for Linux Wayland. Its Wayland implementation writes a screenshot to a temporary
   file before removal, which does not meet the in-memory lifecycle.
4. Use a broad capture crate that includes audio capture dependencies. Screen assistance
   does not need audio capabilities, and importing them would cross into later work.

## Consequences

The feature can answer a new screenshot request or a previous text request without adding
image bytes to Rust session history. The user sees a preview in chat and a separate send
action, while Rust controls the canonical sensitive lifecycle. Platform permission prompts
and available source types vary by desktop backend; an unavailable backend reports that state
rather than silently falling back to another capture method. The five-minute Rust TTL remains
the final cleanup bound even if the UI closes without sending or discarding.
