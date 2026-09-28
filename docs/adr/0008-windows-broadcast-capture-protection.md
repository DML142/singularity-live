# Windows broadcast capture protection

## Status

Accepted — 2026-09-27

## Context

Singularity Live is a backstage assistant for creators. During a live stream or recording,
the creator needs to see and operate the assistant while the audience sees the creator's
content without the assistant window. Hiding the window from the local desktop would interrupt
the workflow. The existing hide/capture/restore sequence serves a different purpose: it keeps
the assistant out of screenshots that Singularity Live captures itself.

## Decision

- Set Tauri's `contentProtected` option on the Windows main window in
  `tauri.windows.conf.json`. The main window remains visible to the creator.
- Treat broadcast capture protection as a core product requirement and a release gate for
  every platform advertised as supported for creator broadcasts.
- Verify the Windows implementation with both display and window capture in OBS before
  claiming it works for broadcasts. Keep hiding the application during its own screenshot
  capture optional; the default leaves it visible and relies on content protection.
- Do not claim equivalent support on Linux, where Tauri's window backend does not implement
  content protection. macOS requires a separate decision about the private API and distribution
  trade-offs, then a dedicated compatibility check.

## Alternatives considered

1. Hide or minimize the main window during broadcasts. This also hides it from the creator and
   makes the assistant harder to use during a live session.
2. Tell creators to crop or reposition their recording source. That makes protection depend on
   per-scene setup and can fail when the capture layout changes.
3. Use one global Tauri setting for all targets. Platform support differs and macOS has a
   private-API trade-off, so a Windows-specific configuration is clearer.

## Consequences

On supported Windows capture paths, the creator can keep the assistant open locally while its
window contents are omitted from the recorded or streamed desktop. Windows documents
`WDA_EXCLUDEFROMCAPTURE` as supported starting with Windows 10 version 2004 and says the
behavior applies through supported public OS capture APIs while DWM composes the desktop.
This is not DRM or a guarantee against every capture implementation; a camera still records
the physical screen, and each advertised recorder/capture mode must pass a manual smoke check.
