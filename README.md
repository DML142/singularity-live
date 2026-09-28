# Singularity Live

Singularity Live is a backstage assistant for creators making live coding streams, recorded
programming or gameplay videos, and dynamic scripts. It accepts typed requests and temporary
screenshots, then returns streamed text through GPT-6 Luna, OpenRouter, or Gemini. Voice input
uses Soniox to create editable text; spoken assistant replies are out of scope. On Windows,
Tauri is configured to exclude the assistant window from supported screen captures while it
remains visible locally; OBS verification is still pending. The product direction and roadmap
are recorded in [tech.md](tech.md).

## Current status

The desktop foundation, manual context/provider path, and transient screenshot assistance
are implemented. Provider calls in automated checks use mocks; live provider requests are
not sent by the test suite. Roadmap status and validation evidence are recorded in
[tech.md](tech.md).

The shell currently provides:

- a live transcript panel and a manual request composer;
- backend readiness loaded through a typed Tauri IPC command;
- temporary screen assistance from the assistant or a configurable global shortcut;
- an always-on-top main window and a Rust-owned capture, permission, and image lifecycle;
- Windows capture-protection configuration for the main window, pending manual OBS validation;
- a Binds settings view for adding screenshot and voice-input shortcuts;
- an AI & keys settings view for selecting a provider profile and opening the local key file;
- microphone or system-audio capture with local silence filtering and manual transcript review;
- Rust-owned context loading, provider configuration, credentials, routing, and streaming;
- safe setup guidance when the selected provider or context pack is unavailable;
- first-run seeding of the example context pack and a key-free default model profile;
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

## Windows executable

Build the current Windows executable without creating an installer:

```powershell
pnpm build:exe
```

The output is `src-tauri/target/release/singularity-live.exe`. Run it and open **Settings →
AI & keys**. Choose a model, save it, then use **Open API key file** to reveal
`provider-settings.json` in Explorer. Add the key under `openaiApiKey`, `openrouterApiKey`,
or `geminiApiKey`, save the JSON file, and refresh the key status. The assistant becomes
available when the selected provider's key is present. `sonioxApiKey` is optional and enables
voice transcription.

The file uses this shape; leave keys for unused providers empty:

```json
{
  "provider": "openai",
  "model": "gpt-6-luna",
  "contextPack": "fictional-developer",
  "openaiApiKey": "",
  "openrouterApiKey": "",
  "geminiApiKey": "",
  "sonioxApiKey": ""
}
```

The settings file is stored in the application's local configuration directory. It contains
plain-text keys, stays on that machine, and is read only by Rust. The webview receives key
presence indicators but never the key values. The app seeds the fictional example context
pack into its application data directory on first launch.

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

For an interactive development launcher with numbered choices for the workflow, provider/model,
and installed context packs. It asks for missing OpenAI, OpenRouter, or Gemini credentials and
an optional Soniox key with hidden input, then passes them only to the app process:

```sh
pnpm assist
```

With arguments, the launcher skips prompts and requires the selected key in the environment.
It never accepts the key as an argument:

```sh
export OPENROUTER_API_KEY='…'
pnpm assist -- --task screenshot --context-pack fictional-developer
```

The direct `pnpm tauri dev` command remains available and unchanged. To choose a model not
listed in the numbered menu, set `SINGULARITY_LIVE_PROVIDER` and `SINGULARITY_LIVE_MODEL`
before running the launcher, or use its argument mode.

## Text, screenshots, and voice input

The default generation profile is GPT-6 Luna for text, reviewed screenshots, code, and
explanations. The app sends requests directly to OpenAI from Rust:

```sh
export SINGULARITY_LIVE_PROVIDER=openai
export SINGULARITY_LIVE_MODEL=gpt-6-luna
export OPENAI_API_KEY='…'
export SINGULARITY_LIVE_CONTEXT_PACK=fictional-developer
pnpm tauri dev
```

For real-time incoming voice transcription, also set `SONIOX_API_KEY` in the local settings
file for the packaged app. `pnpm assist` asks for it optionally with hidden input. The app uses Soniox `stt-rt-v5` for microphone or Windows
system-audio input, then places the finalized transcript in the composer for editing and
manual sending. Without the Soniox key, text and screenshot assistance still work, but voice
input is unavailable. Development environment variables continue to override the local file.

## Alternative text providers

Gemini remains available for text and screenshot assistance. Set the following variables in
the environment inherited by
`pnpm tauri dev`:

```sh
export SINGULARITY_LIVE_PROVIDER=gemini
export SINGULARITY_LIVE_MODEL=gemini-3.8-flash
export GEMINI_API_KEY='…'
export SINGULARITY_LIVE_CONTEXT_PACK=fictional-developer
export SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS=60
pnpm tauri dev
```

Create a key in [Google AI Studio](https://aistudio.google.com/api-keys). The app reads
`GEMINI_API_KEY` only in Rust and sends it to Google's Gemini API as an authorization
header. Do not put it in a `VITE_` variable, frontend `.env` file, source file, or Tauri
command argument. The interactive `pnpm assist` launcher can ask for the key with hidden
input; entering a model ID that starts with `gemini-` selects Gemini automatically.

For OpenRouter, set the following variables instead:

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

The app accepts the selected provider's model identifier in `SINGULARITY_LIVE_MODEL`. The
interactive launcher provides numbered presets and keeps the configured model fixed for that
run. An unset or unsupported provider/model configuration leaves the app open and shows safe
setup guidance. Screenshot assistance sends the reviewed image with the request; speech input
is transcribed to text and assistant replies remain text-only.

Context packs are read from Tauri's platform-specific application data directory at
`context-packs/<SINGULARITY_LIVE_CONTEXT_PACK>/`. With this app's current identifier,
`local.singularity.live`, the base locations are:

- Linux: `$XDG_DATA_HOME/local.singularity.live` or `~/.local/share/local.singularity.live`.
- macOS: `~/Library/Application Support/local.singularity.live`.
- Windows: `%APPDATA%\local.singularity.live`.

These bases come from Tauri's [`app_data_dir()`](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html#method.app_data_dir) API. The app copies its sanitized example pack there on first launch. The pack path for the default profile is:

```text
<app-data>/context-packs/fictional-developer/
├── manifest.yaml
├── answer-style.md
└── sample-projects.md
```

The source for the seeded pack is in [`docs/examples/context-packs/fictional-developer`](docs/examples/context-packs/fictional-developer); the app embeds those files and never reads context from the repository checkout at runtime.

Manifest schema version 1 contains a pack ID, a display name, and at most 16 Markdown
documents. Packs outside app data, symlinked pack directories, unknown fields, invalid or
escaping manifest/document paths, missing files, and oversized content are rejected.
Limits are 32 KiB for the manifest, 64 KiB per document, and 256 KiB for the loaded pack.
Documents marked `always_include` are always selected; other documents are
included in manifest order only when a configured keyword or phrase matches the request.
The 16 KiB request limit is enforced in Rust. Context is sent as reference material in the
system message; the user's text stays in a separate message.

Only one request can be active. Use Cancel to stop the current request; the provider-stream
timeout defaults to 60 seconds and can be set from 5 to 300 seconds. Context preparation
happens before that timeout begins. Provider errors are mapped to safe categories and
messages, with no automatic retry or fallback. Credentials never cross IPC, enter React
state, or appear in logs.

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

Screenshots stay in Rust's bounded in-memory store and expire after five minutes. Capture
starts only after the user presses a configured shortcut or the in-app capture control; the
app never captures in the background or sends an image automatically. Audio remains future
work.

Read [tech.md](tech.md) for the full architecture, roadmap, and current implementation
status. Significant decisions are recorded under [`docs/adr`](docs/adr).

Contribution and Git naming rules are documented in [CONTRIBUTING.md](CONTRIBUTING.md).
