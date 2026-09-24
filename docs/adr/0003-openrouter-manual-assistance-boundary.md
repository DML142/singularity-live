# OpenRouter and the manual-assistance trust boundary

## Status

Accepted — 2026-09-25

## Context

The first real text-generation path needs a low-friction provider configuration while the
application keeps credentials, context files, and vendor protocol details outside the
webview. OpenRouter can route a configurable model slug through one chat-completions API.
This initial capability is a manual text request and does not need fallback policy or a
general-purpose HTTP endpoint.

## Decision

- Implement one direct OpenRouter adapter behind the provider-independent text-generation
  port and Rust-side router.
- Accept a configured OpenRouter model slug, with `openrouter/free` documented as a
  development model-routing option.
- Keep the production endpoint, authorization header, request/response DTOs, and SSE parsing
  inside the OpenRouter adapter.
- Load provider, model, context-pack ID, and bounded timeout from Rust process environment.
- Use a Rust `SecretStore` interface with an environment-backed implementation for local
  development. Do not pass credentials through Tauri IPC or the React runtime.
- Load strict, versioned context packs from Tauri's application-data path. Select documents
  deterministically and send the current user text separately from context.
- Keep one active manual request, keyed by a Rust-generated stable ID, with explicit
  cancellation and a bounded provider-stream timeout.

## Alternatives considered

1. A generic OpenAI-compatible adapter with a user-configurable base URL. This widens the
   network boundary and makes endpoint validation and provider-specific failure behavior
   less precise.
2. Several provider adapters in the first request path. That adds configuration and
   verification breadth before the application needs it.
3. Putting the API key in a frontend settings form or local storage. This would expose a
   provider credential to the untrusted webview.

## Consequences

The first usable provider path has one well-tested network boundary and no speculative
fallback behavior. OpenRouter model availability, behavior, and pricing can change, and the
application does not select or retry a replacement model. The environment-backed secret
source is suitable only for local development; OS-backed credential storage remains future
security work. Automated tests use local mock responses and never require a provider key.
