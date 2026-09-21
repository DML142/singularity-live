# Singularity Live

Singularity Live is a desktop context copilot intended to combine live conversation,
screen context, persistent user context, and configurable AI providers. The current
repository contains the production foundation only: a Tauri 2 desktop shell, a strict
React frontend, one typed frontend-to-Rust status boundary, tests, CI, and the product
architecture.

No audio capture, screenshots, provider calls, API-key handling, or session assistance
is implemented yet. Those capabilities are planned and sequenced in [tech.md](tech.md).

## Current status

Phase 0 (Foundation) is implemented. Its completion status is governed by the validation
record in `tech.md`; later roadmap phases are not started.

The shell currently provides:

- an honest idle workspace for session, transcript, and assistant areas;
- backend readiness loaded through a typed Tauri IPC command;
- a Rust application service that owns the application status response;
- strict, least-privilege defaults with no provider or capture permissions.

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

The webview is treated as an untrusted presentation layer relative to secrets. Future
provider communication, credential storage, capture, persistence, and OS integration all
belong in Rust behind narrow typed commands. React never receives raw provider keys and
does not call model providers directly.

Raw audio and screenshots are planned to be transient by default. Capture will never
start silently on launch, and no stealth or screen-capture-evasion behavior is in scope.

Read [tech.md](tech.md) for the full architecture, roadmap, and current implementation
status. Significant decisions are recorded under [`docs/adr`](docs/adr).
