# Local provider settings for desktop builds

## Status

Accepted — 2026-09-28

## Context

The desktop executable must start without credentials and let its user choose a text provider
and model. The existing React webview is not trusted with API keys, and keys must not cross
Tauri IPC. The settings file also needs to be easy for the user to inspect and edit.

## Decision

- Persist provider, model, context-pack ID, and provider keys in
  `provider-settings.json` under Tauri's application configuration directory.
- Keep provider keys as plain text in that user-owned local file. Rust reads the file through
  the existing `SecretStore` boundary; the settings commands return only key-presence flags.
- Let the webview save only the provider and model profile. A narrow Rust command reveals the
  settings file in the native file manager so the user can edit the keys outside the webview.
- Reload the selected profile and keys for readiness checks and new requests. Environment
  configuration remains available for the development launcher and takes precedence when set.
- Seed the sanitized example context pack into application data on first launch so a packaged
  desktop build has a valid default context without reading from the repository checkout.
- Provide a `build:exe` script that builds the Windows desktop executable without producing an
  installer.

## Alternatives considered

1. Send key input from React to Rust with a save command. This would place the key in an IPC
   request and violate the existing webview credential boundary.
2. Require environment variables for a packaged executable. That prevents normal desktop
   users from configuring the app without a terminal.
3. Add a system credential-vault dependency now. The initial desktop build needs an editable
   local file, and vault integration is a separate platform-security decision.

## Consequences

The webview can configure models and see whether each key is present without receiving key
values. Plain-text key storage is convenient and editable. The settings file is created with
owner-only permissions on Unix and inherits the application config directory's access controls
on Windows. Users and software with access to the account can still read it. Operating-system-
backed credential storage remains future work. Provider file changes are read for new
readiness checks and requests.
