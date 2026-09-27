# Global screenshot shortcuts and Rust-owned capture lifecycle

## Status

Accepted — 2026-09-27

## Context

Screen assistance already has an explicit, temporary capture and send flow, but reaching it
requires opening the application. Users need a configurable global shortcut that works while
the window is minimized. Shortcuts are operating-system integration, while captured screen
content must retain the existing permission, preview, cancellation, and deletion boundaries.
Linux Wayland does not expose the X11 global-shortcut backend used by the Tauri plugin.

## Decision

- Keep shortcut validation, registration, persistence, activation, monitor resolution,
  capture, window hide/restore, cancellation, and image cleanup in Rust.
- Use Tauri's global-shortcut plugin on Windows, macOS, and Linux X11. Use the XDG
  GlobalShortcuts portal on Linux Wayland and report the effective trigger only after portal
  confirmation. Its user-visible consent/configuration behavior is authoritative.
- Persist only a versioned list of shortcut bindings under the application configuration
  directory (falling back to the application data directory). Store no image bytes or
  credentials in this configuration.
- On shortcut activation, reject overlapping work, hide the main window, capture one
  monitor frame, restore the window with always-on-top enabled, then publish the existing
  temporary preview. Capture errors, cancellation, or restore failures clear transient image
  data and produce safe UI state. Capture never sends automatically.
- Keep React limited to explicit binding edits, chord recording, and state/preview
  presentation. The development launcher passes a non-echoed key only to the child process
  and never writes it to disk.

## Consequences

The feature reuses the existing screenshot lifecycle and provider boundary without adding
image persistence or a second capture path. Native hotkeys work only while the application
process is running. Wayland may require user-visible portal consent and can choose an
effective trigger different from the requested one. Wayland sessions without a
GlobalShortcuts portal cannot register binds; users need a supported portal backend or an
X11 session. Cross-platform desktop smoke checks are required before this roadmap item is
marked complete.
