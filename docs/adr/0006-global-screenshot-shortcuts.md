# Global action shortcuts and Rust-owned capture lifecycle

## Status

Accepted — 2026-09-27

## Context

Screen assistance already has an explicit, temporary capture and send flow, but reaching it
requires opening the application. Voice input also needs an explicit start/stop action while
the creator is recording. Users need configurable global shortcuts that work while the window
is minimized. Shortcuts are operating-system integration, while captured screen and audio
content must retain their existing permission, preview, cancellation, and deletion boundaries.
Linux Wayland does not expose the X11 global-shortcut backend used by the Tauri plugin.

## Decision

- Keep shortcut validation, registration, persistence, and action dispatch in Rust. Screenshot
  activation uses the existing monitor resolution, capture, window hide/restore, cancellation,
  and image cleanup flow. Voice-input activation starts or stops the Rust-owned transcription
  service without hiding the window. Quick-send activation emits a narrow event to the mounted
  composer, which sends the current draft through the existing manual-assistance command
  without restoring a minimized window.
- Use Tauri's global-shortcut plugin on Windows, macOS, and Linux X11. Use the XDG
  GlobalShortcuts portal on Linux Wayland and report the effective trigger only after portal
  confirmation. Its user-visible consent/configuration behavior is authoritative.
- Persist only a versioned list of shortcut bindings under the application configuration
  directory (falling back to the application data directory). Store no image bytes or
  credentials in this configuration.
- Screenshot activation rejects overlapping work, hides the main window, captures one
  monitor frame, restores the window with always-on-top enabled, then publishes the existing
  temporary preview. Capture errors, cancellation, or restore failures clear transient image
  data and produce safe UI state. Capture never sends automatically.
- Voice-input activation toggles explicit microphone or system-audio transcription. It never
  submits a transcript to text generation automatically. Quick send only submits the current
  composer draft and is a separate configurable action.
- Keep React limited to explicit binding edits, chord recording, and state/preview
  presentation. The development launcher passes a non-echoed key only to the child process
  and never writes it to disk.

## Consequences

The feature reuses existing capture and voice-input services without adding image/audio
persistence or a second capture path. Native hotkeys work only while the application process
is running. Quick send relies on the mounted WebView composer receiving the event; it does not
raise or restore the main window. Wayland may require user-visible portal consent and can choose an effective trigger
different from the requested one. Wayland sessions without a GlobalShortcuts portal cannot
register binds; users need a supported portal backend or an X11 session. Cross-platform desktop
smoke checks are required before this roadmap item is marked complete.
