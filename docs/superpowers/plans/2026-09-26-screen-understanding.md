# Screen Understanding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit transient screenshot assistance to the Rust-owned conversation and provider flow.

**Architecture:** Rust exposes narrow capability, capture, crop, discard, and send actions. A one-shot XCap adapter serves Windows, macOS, and X11; Linux Wayland uses its explicit XDG ScreenCast picker and receives one in-memory PipeWire frame. Both feed a bounded in-memory image store. The existing session service attaches one temporary image to the current user message and retains only completed text. React starts actions and displays state and preview.

**Tech Stack:** Rust 1.98, Tauri 2, XCap 0.9, XDG portal (`ashpd`) and PipeWire on Linux Wayland, `image`, `base64`, React 19, strict TypeScript, Vitest, mock Rust providers.

**Spec:** `docs/superpowers/specs/2026-09-26-screen-understanding-design.md`

## Global Constraints

- Node.js 24 LTS, pnpm 12.5.1, Rust 1.98.0.
- Rust owns capture, permission handling, image processing, context selection, provider calls, cancellation, and deletion.
- Capture starts only after a visible user action; there is no background capture.
- Images are held only in memory for at most five minutes and are removed on all terminal paths.
- Never persist or log screenshot bytes, provider keys, authorization headers, or full provider payloads.
- Keep OpenRouter behind the provider-neutral Rust boundary; normal tests use mocks and never send live requests.
- Never use file-based Wayland screenshot paths, including a temporary path that is immediately deleted.
- Do not implement audio, VAD, STT, transcription, OCR, SQLite, or persistent image storage.
- Keep Phase 2 and Phase 3 statuses `Not started` until their own acceptance criteria pass; Phase 4 remains `In progress`.

## Review Focus

- A capture ID from another session or after expiry must be rejected without exposing image bytes.
- Cancellation at provider start, stream, and terminal delivery must release image bytes and the active request slot.
- Unsupported desktop backends and OS permission denial must map to distinct safe UI states.
- Crop bounds must reject empty, negative, overflowed, and out-of-image rectangles.
- A text-only follow-up must preserve the current request wire shape and prior role-tagged turns.

---

### Task 1: Capture capability, preparation, and transient storage

**Files:**

- Create: `src-tauri/src/capture/mod.rs`
- Create: `src-tauri/src/capture/backend.rs`
- Create: `src-tauri/src/capture/image.rs`
- Create: `src-tauri/src/capture/store.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/lib.rs`
- Test: capture module unit tests

**Interfaces:**

- Produces `CaptureBackend::capabilities`, `CaptureBackend::targets`, and
  `CaptureBackend::capture(target)`, plus typed `CaptureError` and `CaptureCapabilities`.
- Produces `TransientImageStore::insert`, `preview`, `take`, `discard`, `purge_expired`,
  and `clear`; it stores normalized PNG bytes in memory and exposes no path API.
- XCap access is isolated in `XCapCaptureBackend`; Linux Wayland uses a `PortalCaptureBackend`
  which selects exactly one monitor/window, disables portal persistence, reads one PipeWire
  video frame, then closes the stream and portal session. Tests use fake backends.

- [x] Write tests for only-on-request capture, unsupported capability, permission failure,
      invalid crop, in-memory preview, replacement, expiry, discard, and reset deletion.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml capture` and confirm each new
      behavior fails because its implementation is absent.
- [x] Add XCap, portal, PipeWire, and image-preparation code that makes those tests pass; keep pixel bytes out
      of `Debug` output and logs.
- [x] Run the targeted Rust tests, formatting, and Clippy.
- [x] Commit as `feat: add transient screenshot capture`.

### Task 2: Multimodal session request and OpenRouter mapping

**Files:**

- Modify: `src-tauri/src/domain/generation.rs`, `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/providers/openrouter.rs`
- Modify: `src-tauri/src/app/session.rs`, `src-tauri/src/context/session.rs`
- Test: `src-tauri/tests/openrouter_adapter.rs`, `src-tauri/tests/manual_assistance.rs`

**Interfaces:**

- Produces provider-neutral `MessagePart::Text` and `MessagePart::Image`; image bytes are
  request-scoped and have a redacted `Debug` representation.
- Produces `SessionService::start_screenshot(prepared_image, sink)`; the IPC command first
  consumes the opaque capture ID from the transient store and moves the image into the request.
  The service then
  preserves completed text turns, selects context from prior user intent, and records only
  text after success.
- OpenRouter serializes only multimodal user messages as ordered text/image content parts;
  existing text-only message JSON stays a string.

- [x] Write mock-router tests for prior user/assistant text plus a current image, text-only
      request compatibility, image removal after completion/failure/cancellation, and no image
      bytes in session history or debug output.
- [x] Run the targeted Rust tests and confirm the new cases fail before implementation.
- [x] Implement provider-neutral parts and the OpenRouter base64 data URL mapping. The
      existing router port already accepts provider-neutral conversation messages, so its
      interface did not need to change.
- [x] Run targeted Rust tests, formatting, and Clippy.

### Task 3: Typed IPC, permissions, error states, and cancellation

**Files:**

- Create: `src-tauri/src/commands/screen_assistance.rs`
- Create: seven narrow permission files under `src-tauri/permissions/autogenerated/`
- Modify: `src-tauri/build.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/main-window.json`
- Modify: `src-tauri/tests/command_permissions.rs`
- Test: `src-tauri/src/commands/screen_assistance.rs`

**Interfaces:**

- Produces narrow commands for capability query, explicit capture, validated region crop,
  discard, provider send, and capture cancellation. All payloads deny unknown fields and use
  opaque capture IDs.
- Reuses the existing cancellation command for an active screenshot-assisted provider request.
- IPC maps internal errors to safe capability/error codes and static messages.

- [x] Write tests for allowlisted commands, rejected unknown payload fields, safe error
      mapping, expired IDs, and cancellation cleanup; verify the permission test fails first.
- [x] Implement generated permissions and register commands in the Rust build manifest.
- [x] Run targeted Rust tests, formatting, and Clippy.

### Task 4: Explicit preview UI and validation records

**Files:**

- Create: `src/lib/tauri/screen-assistance-client.ts`
- Create: `src/lib/tauri/screen-assistance-client.test.ts`
- Modify: `src/stores/manual-assistance-store.ts`
- Modify: `src/features/assistant/AssistantPanel.tsx`, `src/features/assistant/AssistantPanel.test.tsx`
- Modify: `src/app/styles/global.css`
- Create: `docs/adr/0005-ephemeral-screen-assistance.md`
- Modify: `docs/adr/README.md`, `tech.md`

**Interfaces:**

- Produces typed client methods that validate unknown IPC responses at runtime.
- UI state covers unavailable, capturing, preview, sending, failed, cancelled, discarded,
  and expired. Send requires a second explicit click after preview.
- The assistant store never logs or persists the preview; in-memory preview data is cleared
  when replaced, sent, discarded, reset, expiry, or unmount.

- [x] Write frontend tests for explicit action order, preview send/discard, capability and
      permission states, errors, cancellation, and preview release; confirm they fail first.
- [x] Implement the client, minimal controls, preview, visible status copy, and cleanup.
- [x] Add the ADR and update `tech.md`: record the user's Phase 1 OpenRouter verification as
      completed; record Phase 2 only if its acceptance criteria pass; retain Phase 3 as `Not
started` and Phase 4 as `In progress`.
- [x] Run frontend tests, `pnpm check`, `git diff --check`, and the Phase 2 acceptance
      checklist with a mock provider and no live network call. `pnpm check` passed on
      2026-09-26 with 25 frontend and 94 Rust tests.
- [x] Commit the complete, cross-boundary feature as `feat: add explicit screenshot assistance`.
