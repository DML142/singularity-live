# Screen Understanding Design

**Status:** Approved for implementation on 2026-09-26

## Goal and boundaries

Add explicitly initiated, transient screenshot assistance to the current Rust-owned
session and provider flow. The user chooses a supported screen or window, may select a
region from the preview, reviews the prepared image, and explicitly sends it with the
existing text conversation context.

Capture, platform permission handling, image preparation, context selection, provider
calls, cancellation, and image deletion remain in Rust. React may request a capability
report, start an allowed capture or send action, select a crop rectangle, and render status
and preview. No image is captured on startup or in the background.

## Capture and image lifecycle

- Put platform calls behind a Rust `CaptureBackend`. Windows, macOS, and X11 use XCap's
  one-shot monitor, window, and region APIs. Linux Wayland uses the XDG ScreenCast portal
  for its visible source picker and a single PipeWire video frame. Do not call XCap's
  Wayland adapters: they write screenshots to a temporary file before removing it. Report
  unsupported or unavailable platform paths instead of falling back to hidden capture.
- Query capabilities and target metadata through typed Rust commands. Capture occurs only
  after a user action. Region coordinates come from an explicit crop selection in the
  displayed preview; Rust validates and applies the crop to the captured image.
- Keep one prepared image in a Rust-owned in-memory store with an opaque capture ID and a
  five-minute expiry. Do not write raw or prepared screenshots to disk or logs.
- Return a short-lived in-memory preview representation for display. The UI keeps it only
  in component/store memory and drops it on send, discard, reset, expiry, or unmount. Rust
  removes canonical image bytes on send, discard, reset, expiry, provider completion,
  provider error, cancellation, and process exit.
- Bound dimensions and encoded size during Rust image preparation. Do not add OCR or retain
  an image in conversation history or rolling summaries.

## Session and provider flow

The Rust session service consumes the capture ID and builds one current user message from
the previous role-tagged text turns, a short instruction to continue the previous request,
and the image. Context selection uses the prior user intent. A completed exchange stores
only text; the raw image is released. OpenRouter receives provider-neutral image content
mapped to its image URL content part with a local base64 data URL. Text-only requests keep
their existing wire shape. Provider/model rejection is returned as a safe visible error.

The screenshot flow has explicit capability, permission, capture, preview, sending,
failed, cancelled, discarded, and expired states. Sending after preview requires a separate
user action. Existing Rust cancellation owns provider request cancellation and image
cleanup. All ordinary provider tests use a mock; no live provider request is made.

## Errors and validation

Use typed Rust errors for unsupported capture, permission required or denied, invalid
target/region, expired capture, busy session, preparation failure, provider rejection,
provider failure, and cancellation. IPC responses expose only safe codes and messages.
Tests cover explicit permission/capture calls, capability permissions, image expiry and
deletion, crop validation, session-context plus image composition, provider serialization,
error mapping, and cancellation cleanup. Sensitive image bytes and provider payloads never
enter logs.

## Non-goals

Audio, VAD, STT, transcription, OCR, persistent image/history storage, background capture,
automatic sending, SQLite, and other Phase 3 or later work.

## Backend choice

XCap is chosen for its direct still-image API and monitor/window coverage on Windows,
macOS, and X11. Its Linux Wayland adapter was excluded because it writes screenshot files
to a temporary directory. The Wayland adapter instead uses the XDG ScreenCast portal with
`PersistMode::DoNot`, a one-source selection, and one PipeWire frame; session teardown
immediately ends the stream. Region selection is a validated in-memory crop of the prepared
frame. The approved alternative `scap` was not selected because its crate dependency set
includes audio capture (`cpal`), outside this phase's scope.
