# Global Shortcuts and Binds Settings

**Status:** Draft for user review

**Date:** 2026-09-27
**Planning label:** Proposed Phase 2.5 follow-up to transient screen assistance

## Goal

Let the user trigger the existing transient screenshot flow from a global keyboard
shortcut while Singularity Live is running, whether the main window is active or
minimized. Keep the main window above other application windows and provide an in-app
settings view for managing shortcut bindings. Provide an optional development-only CLI
launcher that avoids repeated shell setup without saving credentials.

## User-approved behavior

- Global bindings stay registered while the application process is running, including
  while its window is minimized. Closing the application ends the listener; system
  autostart and a tray-resident process are outside this feature.
- The main application window is always-on-top. The default binding is
  `Ctrl + Win + P` and runs the `Screenshot` action on Windows. The equivalent platform
  modifier is displayed on other operating systems.
- A hotkey captures the monitor under the pointer. The user's current setup has one
  monitor. Linux Wayland continues to use its user-visible screen-capture portal; with
  multiple displays, the portal's explicit source selection takes precedence over querying
  the pointer's monitor.
- Before capture, the application window is hidden briefly so the capture does not include
  Singularity Live itself. The window is then restored, brought forward, and kept
  always-on-top with the existing screenshot preview. A capture error also restores the
  window and displays a safe error.
- Capturing never sends automatically. The existing preview, crop, explicit send, discard,
  and five-minute transient image lifecycle remain in effect.
- Settings contain a Binds tab. There is at least one binding and no application-imposed
  maximum number of rows. A row maps one shortcut to one action; multiple rows may map to
  the same action. `Screenshot` is the only action in this feature.
- Settings are a dedicated view in the existing main Tauri window, not a second native
  window. The view contains a Binds tab; model and credential management remain out of
  scope.
- A binding is an OS-supported chord of one to three simultaneously pressed keys (modifier
  keys plus at most one trigger key, not a sequence). Recording is activated by the row's
  capture control; Escape clears that row's combination and cannot be assigned as a
  shortcut. A separate row control removes the binding.
- Duplicate chords are rejected. If the OS or another application prevents registration,
  the affected binding reports an error and the user can choose another chord.
- The developer CLI is optional and does not persist API keys. With no arguments it offers
  choices for implemented development tasks and asks for missing non-secret configuration.
  If the key is missing, it prompts with hidden input and passes it only to the launched
  development process. If arguments are supplied, the task/configuration menu is skipped;
  non-secret configuration comes from arguments or the environment, and the key must
  already be in the environment. The key is not accepted as a command-line argument.
  Direct `pnpm tauri dev` remains available and uses the current environment configuration.
- Model selection and persistent on-device credential storage belong to a future product
  settings feature.

## Architecture and data flow

The existing Tauri process owns shortcut registration and dispatch. Rust loads and
validates the binding configuration, registers shortcuts through the Tauri desktop
global-shortcut plugin, and routes a `Screenshot` activation to the existing Rust capture
service. The settings view and Binds tab live in the main window. React presents settings,
collects a chord only while a bind row is explicitly in recording mode, and displays
registration, capture, permission, and error states. React does not register OS shortcuts
or orchestrate capture.

On shortcut activation, Rust resolves the monitor under the pointer using a platform
adapter, hides the main window, invokes the existing explicit one-frame capture path, and
returns the temporary preview to the existing screen-assistance UI. The application then
restores the window's visibility, focus, and always-on-top state. On Linux Wayland the
XDG ScreenCast portal remains the authority for capture selection and consent; the
single-monitor setup means its selected monitor is unambiguous.

The monitor/cursor resolver, global shortcut registration, window lifecycle, and settings
file remain on the Rust side of the Tauri boundary. Typed commands let the settings UI
load/update bindings and let the capture UI receive the preview state. Any image bytes
continue to use the existing request-scoped in-memory lifecycle; shortcut settings never
contain image or credential data.

## Binding settings and persistence

Bindings are stored in a small versioned, non-secret configuration file beneath Tauri's
per-user application configuration directory. They are independent of the context pack and
provider environment. The default chord is represented with the platform's equivalent
super key: Windows displays `Ctrl + Win + P`, Linux displays `Ctrl + Super + P`, and macOS
displays `Control + Command + P`. If no binding configuration exists, this
platform-specific default is assigned to `Screenshot`.

The Settings view in the main window provides:

- Add binding and remove-row controls, with at least one row retained.
- A capture control per row that records one to three simultaneous keys; Escape clears the
  row's chord without closing settings.
- An action selector, currently containing only `Screenshot`.
- A clear indication that a binding is being recorded and a safe, per-row registration
  failure state.

The Rust settings service validates chord shape, supported keys, duplicate bindings, and
action identifiers before registration or persistence. Updating the configuration is
transactional: if a requested change cannot be registered, the previously working
configuration remains active and is not overwritten. On startup, a persisted binding that
cannot be registered is reported in settings while other valid bindings remain usable.

## Runtime and failure behavior

- A hotkey press while another capture or manual request is active does not start a second
  capture or request. The app reports its busy state in the restored window.
- Shortcut registration conflicts, unsupported key codes, inaccessible screen sources,
  denied permissions, capture failures, and window restore failures are mapped to safe UI
  states. Screenshot bytes, API keys, and full provider payloads are not logged.
- If capture is cancelled or fails after hiding the window, the application restores the
  window and deletes any transient image. Existing five-minute expiry remains the final
  cleanup bound.
- `Ctrl + Win + P` is a default proposal, not a guarantee that every desktop environment
  leaves the chord available. A registration conflict is visible and can be fixed by
  editing the binding.

## Development CLI

The CLI is a developer launcher, separate from product settings and not required for
normal desktop operation. It offers only implemented development tasks (manual text and
screenshot assistance). Selecting a task configures the launch experience but never
captures or sends a screenshot by itself. Without arguments, non-secret configuration uses
existing environment values or documented development defaults and the helper asks for
missing task/model choices. The context pack must already be configured and installed; the
helper does not copy or overwrite user context files. With arguments, task/model/context-pack
values must be supplied by arguments or the environment, or use documented defaults where
available. The OpenRouter key is read from the environment when present, otherwise
accepted through non-echoing input for the child process only. In argument mode, a missing
key is reported and the helper exits instead of prompting. The key is never written to a
file, printed, or accepted as a command-line argument.

The existing direct `pnpm tauri dev` workflow remains unchanged for developers who already
provide all required environment values. The CLI's argument names and executable/script
name are implementation details to settle in the implementation plan.

## Acceptance criteria

1. With the app running and its window active, the default global shortcut starts one
   explicit capture. With the window minimized, the same shortcut starts capture and
   restores the app to show the preview.
2. The main window is always-on-top during normal use and after restore. It is absent from
   the captured image when it was visible at shortcut activation.
3. On a multi-monitor platform adapter test, cursor coordinates resolve to the containing
   monitor. On the user's single-monitor setup and Linux Wayland portal path, the selected
   source is that monitor and consent remains user-visible.
4. Users can add and remove any number of binding rows while retaining at least one;
   record one-, two-, and three-key chords; clear a chord with Escape; select the action;
   and map several different chords to `Screenshot`.
5. Duplicate, unsupported, or OS-conflicting chords produce a clear error without losing
   previously working bindings. Bind configuration survives application restart.
6. Screenshot preview remains temporary, does not enter history or logs, and is not sent
   until the existing explicit send action. Busy, cancellation, permission, and capture
   error paths restore the window and delete transient image data.
7. The dev CLI's no-argument path offers task choices and can accept a key without echoing
   or persisting it. Supplying arguments skips prompts and requires the key in the
   environment; missing configuration fails clearly. The existing direct `pnpm tauri dev`
   path continues to work.
8. Automated tests cover binding parsing/validation, configuration persistence, shortcut
   registration and rollback using a mock registrar, cursor-to-monitor resolution,
   window hide/capture/restore ordering, error and cancellation cleanup, CLI behavior, and
   secret non-echo/non-persistence. Tests use mock capture/providers and never make live
   provider requests.
9. A manual desktop smoke check on each supported OS validates active and minimized
   hotkey activation, always-on-top behavior, capture without self-inclusion, and the
   platform permission flow before the roadmap status is marked complete.

This follow-up does not implement audio or transcription and does not change the Phase 3
`Not started` or Phase 4 `In progress` statuses.

## Out of scope

- Background or automatic screen capture, automatic provider submission, OCR, audio,
  transcription, VAD, persistent screenshot storage, and SQLite.
- A tray-resident process after the user exits the application, launch-at-login, and a
  separate hotkey daemon.
- Product UI for model selection or persistent credential storage.
- Passing API keys through command-line arguments, writing keys to `.env` or plain-text
  configuration, or sending live requests during automated checks.
- Actions other than `Screenshot`; the action type may be extended when a later capability
  is implemented.
