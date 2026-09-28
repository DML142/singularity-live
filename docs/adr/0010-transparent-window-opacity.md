# Transparent window opacity customization

## Status

Accepted — 2026-09-28

## Context

Creators keep Singularity Live visible while working in a code editor or recording. They need
to see content behind the assistant window without hiding the assistant. The app also has a
Windows capture-protection setting, so the window must stay visible to the creator while its
contents remain excluded from supported recording paths.

## Decision

- Enable Tauri window transparency in the Windows-specific configuration and keep the WebView
  document background transparent.
- Apply the selected opacity to the complete app shell so both the chat and settings surface
  reveal the desktop behind them consistently. Use fixed five-percent slider steps from 40% to
  100%.
- Persist the selected value as a versioned Rust-owned setting under the application
  configuration directory, falling back to the application data directory. Expose only typed
  get and set commands; do not use browser storage.
- Keep `contentProtected` enabled on the Windows main window. Opacity changes the creator's
  local view and does not change the broadcast-capture protection policy.

## Alternatives considered

1. Change each panel background color independently. This would require every nested panel to
   track the setting and could leave opaque surfaces behind.
2. Fade the window through a broad operating-system API. Tauri does not expose a portable
   whole-window opacity setter, while the transparent WebView surface already supports the
   requested visual effect.
3. Store the setting in browser storage. That would put application preference persistence in
   React instead of the Rust-owned persistence boundary.

## Consequences

Opacity is adjustable at runtime and survives application restarts. At lower values, text and
controls fade along with their backgrounds. The native window's system-drawn frame may remain
opaque. Transparency behavior still needs visual verification on the Windows desktop build.
