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
- Provide configurable Screenshot, Send screenshot, Capture and send screenshot, and
  Hide/show taskbar icon actions. The taskbar action changes taskbar visibility while leaving
  the application window in place. Send screenshot consumes only the current reviewed
  preview and is a no-op when no preview is ready.
- Capture and send screenshot uses `Ctrl+Shift+Enter` by default. Rust performs the existing
  capture lifecycle and marks a successful preview for immediate submission; the mounted
  composer sends it with the current draft or the default screenshot prompt. The ordinary
  Screenshot action continues to stop at the review preview.
- Keep a native system-tray icon and Show/Hide/Quit menu available while the window is hidden
  or closed. A left click restores the window; the operating system decides whether the icon
  appears directly in the notification area or in its overflow menu.
- Keep the selected voice source and microphone endpoint in a separate versioned Rust-owned
  settings file. The settings screen selects the source; the composer starts and stops capture.
- Screenshot activation rejects overlapping work and uses the saved monitor/window source
  preference. It hides and restores the main window only when the close-on-screenshot setting
  is enabled; this setting is off by default. Capture errors, cancellation, or restore failures
  clear transient image data and produce safe UI state. The Screenshot action never sends
  automatically; Capture and send screenshot is a separate explicit shortcut action.
- Voice-input activation toggles explicit microphone or system-audio transcription. It never
  submits a transcript to text generation automatically. Quick send only submits the current
  composer draft and is a separate configurable action.
- Keep React limited to explicit binding edits, chord recording, and state/preview
  presentation. The development launcher passes a non-echoed key only to the child process
  and never writes it to disk.

## Consequences

The feature reuses existing capture and voice-input services without adding image/audio
persistence or a second capture path. The assistant can remain visible during capture on
Windows where content protection excludes it from supported screen-capture APIs. Native hotkeys
and the tray icon work only while the application process is running. Quick send and screenshot
send rely on the mounted WebView composer receiving events; neither implicitly captures or
restores a minimized window. Wayland may require user-visible portal consent and can choose an effective trigger different from the
requested one. Wayland sessions without a GlobalShortcuts portal cannot
register binds; users need a supported portal backend or an X11 session. Cross-platform desktop
smoke checks are required before this roadmap item is marked complete.

## Amendment — explicit screenshot capture and send (2026-09-28)

The creator requested a single explicit shortcut for capturing and immediately submitting a
screenshot. This is separate from both preview-only Screenshot and Send screenshot, so
existing bindings keep their behavior. The default is `Ctrl+Shift+Enter`; it uses the same
Rust-owned capture, cancellation, visibility, and transient-image lifecycle. A successful
capture is submitted by the mounted composer, and a failed capture is never sent.
