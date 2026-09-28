# Screenshot messages and minmode

## Status

Accepted — 2026-09-28

## Context

The first screenshot flow showed a separate capture panel above chat and required a prior text
request. Creators need to review a screenshot in the conversation, crop it there, add a note,
and optionally choose one short instruction. The application also needs a configurable compact
presentation while keeping the existing Rust-owned capture and global shortcut boundaries.

## Decision

- Keep screenshot bytes in Rust's transient image store for at most five minutes. The
  conversation may display the in-memory preview until that original expiry, session reset, or
  view unmount; it is never persisted or included in Rust session history.
- Send a screenshot as the current user turn with its note and image. A previous text turn is
  optional. When no note or context action is selected, use a short screenshot description
  prompt.
- Offer one context action at a time: `explain`, `tell me more`, or `fix`. Show its prefix as
  non-editable muted text in the composer and include it in the user text sent to Rust.
- Store the screenshot source (`monitor` or `window`) and the optional close-on-capture choice
  in Rust-owned customization settings. Default close-on-capture to off. On native desktops,
  resolve the selected source under the pointer; on Wayland, keep the existing portal picker.
- Use a configurable global `MinMode` shortcut to toggle frontend presentation. The mode
  hides the app header, panel header, and session controls while keeping the conversation and
  composer mounted.

## Consequences

Screenshot notes, context prefixes, and attached images are composed into one provider request.
Completed Rust session history continues to retain text only. A small base64 preview can remain
in frontend memory until the screenshot's existing expiry; reset and unmount release it sooner.
With close-on-capture disabled, Windows content protection can keep the assistant visible while
supported desktop capture APIs omit its window. Other platforms retain their existing capture
limitations.

Minmode is presentation state for the current app process and resets when the application
restarts. Its global shortcut registration and persistence use the existing shortcut settings
service.
