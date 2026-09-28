# Audio-source, click-through, and application-scale controls

## Status

Accepted — 2026-09-28

## Context

Creators need to change between microphone and system-audio transcription without opening
settings, temporarily let mouse clicks pass through the assistant window, and adjust the app's
content size independently from window dimensions. These actions use existing Rust-owned
shortcut and customization boundaries.

## Decision

- Add global shortcut actions to switch the persisted voice source and toggle native
  click-through for the main window. Source changes apply to the next recording; an active
  recording is left intact and the UI reports that the source could not be changed.
- Keep click-through as process state. Its shortcut remains available when pointer events are
  ignored, so the same bind restores normal interaction. The setting starts disabled after
  application restart.
- Persist application scale with Rust-owned customization settings and apply it to the Tauri
  webview as browser-style zoom. Offer 70%–130% in 10% steps, defaulting to 100%.
- Preserve existing customization files by defaulting the newly added scale field when it is
  absent.

## Consequences

Shortcut registration and OS window changes remain in Rust. The voice-source setting stays in
sync with Settings through typed voice-input events. Webview zoom is applied when saved and on
startup. Mouse click-through requires a global shortcut to turn off while enabled.
