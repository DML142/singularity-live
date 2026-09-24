# Singularity Live

Singularity Live is a desktop context copilot for working with live conversation, screen
context, persistent user context, and configurable AI providers. The app now supports
manual text assistance through OpenRouter, with context packs loaded by Rust from the
application data directory. Audio, screenshots, session history, and persistent credential
storage remain future work; the implementation record is in [tech.md](tech.md).

## Current status

The desktop foundation and manual context/provider path are implemented. The provider path
has automated coverage; a live OpenRouter request has not been verified in this workspace.
The roadmap status and validation evidence are recorded in [tech.md](tech.md).

The shell currently provides:

- an honest idle workspace for session and transcript areas, plus a manual request composer;
- backend readiness loaded through a typed Tauri IPC command;
- Rust-owned context loading, provider configuration, credentials, routing, and streaming;
- safe setup guidance when OpenRouter or the configured context pack is unavailable;
- exact Tauri permissions for status, readiness, start, and cancel commands.

## Technology

- Tauri 2 and Rust 2024 edition
- React 19, TypeScript, and Vite
- Tailwind CSS and Zustand
- Vitest and React Testing Library
- pnpm

Exact resolved dependency versions are captured in `pnpm-lock.yaml` and
`src-tauri/Cargo.lock`. The toolchain versions validated during bootstrap are recorded in
[tech.md](tech.md#toolchain-and-tested-versions).

## Prerequisites

- Node.js 24 LTS
- pnpm 12 (the exact package-manager version is declared in `package.json`)
- Rust 1.98 with `rustfmt` and `clippy` (declared in `rust-toolchain.toml`)
- the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/) for
  your operating system

On Ubuntu 24.04, the native development packages used by CI are listed in
`.github/workflows/ci.yml`.

## Development

Install JavaScript dependencies:

```bash
pnpm install
```

Run the browser-only frontend during UI work:

```bash
pnpm dev
```

Run the desktop application with the real Rust IPC boundary:

```bash
pnpm tauri dev
```

## Manual text assistance

The first provider is OpenRouter. Set the following variables in the environment inherited
by `pnpm tauri dev`:

```sh
export SINGULARITY_LIVE_PROVIDER=openrouter
export SINGULARITY_LIVE_MODEL=openrouter/free
export SINGULARITY_LIVE_CONTEXT_PACK=fictional-developer
export SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS=60
pnpm tauri dev
```

Set `OPENROUTER_API_KEY` in the same desktop process environment using your local secret
manager or shell environment setup. The app reads it only in Rust. Do not put it in a
`VITE_` variable, frontend `.env` file, source file, or Tauri command argument. The included
`openrouter/free` model slug routes each request to a currently available free model, so the
underlying model may vary between requests. Availability and behavior can change; see
[OpenRouter's free router](https://openrouter.ai/openrouter/free) and
[model catalog](https://openrouter.ai/models).

The app accepts an OpenRouter model slug in `SINGULARITY_LIVE_MODEL`. It does not provide a
model picker or automatically change models. An unset or unsupported provider/model
configuration leaves the app open and shows safe setup guidance.

Context packs are read from Tauri's platform-specific application data directory at
`context-packs/<SINGULARITY_LIVE_CONTEXT_PACK>/`. With this app's current identifier,
`local.singularity.live`, the base locations are:

- Linux: `$XDG_DATA_HOME/local.singularity.live` or `~/.local/share/local.singularity.live`.
- macOS: `~/Library/Application Support/local.singularity.live`.
- Windows: `%APPDATA%\local.singularity.live`.

These bases come from Tauri's [`app_data_dir()`](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html#method.app_data_dir) API. Copy the sanitized example into the configured directory, for example:

```text
<app-data>/context-packs/fictional-developer/
├── manifest.yaml
├── answer-style.md
└── sample-projects.md
```

The repository example is in [`docs/examples/context-packs/fictional-developer`](docs/examples/context-packs/fictional-developer). Copy that directory's contents into the runtime path; the app never reads context from the repository checkout.

Manifest schema version 1 contains a pack ID, a display name, and at most 16 Markdown
documents. Unknown fields, invalid or escaping paths, missing files, and oversized content
are rejected. Limits are 32 KiB for the manifest, 64 KiB per document, and 256 KiB for the
loaded pack. Documents marked `always_include` are always selected; other documents are
included in manifest order only when a configured keyword or phrase matches the request.
The 16 KiB request limit is enforced in Rust. Context is sent as reference material in the
system message; the user's text stays in a separate message.

Only one request can be active. Use Cancel to stop the current request; the default total
timeout is 60 seconds and can be set from 5 to 300 seconds. Provider errors are mapped to
safe categories and messages, with no automatic retry or fallback. Credentials never cross
IPC, enter React state, or appear in logs.

Common validation commands:

```bash
pnpm format
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm rust:check
pnpm check
```

`pnpm check` runs the complete local validation sequence. Linux requires the Tauri native
development packages before Rust checks can compile the webview runtime.

## Architecture and privacy

The webview is treated as an untrusted presentation layer relative to secrets. Provider
communication, credential lookup, context loading, capture, persistence, and OS integration
belong in Rust behind narrow typed commands. The environment-backed credential source is
for local development; React never receives provider keys and does not call model providers
directly.

Raw audio and screenshots are planned to be transient by default. Capture will never
start silently on launch, and no stealth or screen-capture-evasion behavior is in scope.

Read [tech.md](tech.md) for the full architecture, roadmap, and current implementation
status. Significant decisions are recorded under [`docs/adr`](docs/adr).

Contribution and Git naming rules are documented in [CONTRIBUTING.md](CONTRIBUTING.md).
