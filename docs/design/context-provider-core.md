# Context and provider core design

## Purpose

Add a narrow manual-text assistance path without weakening the existing trust boundary.
React remains an untrusted presentation layer. Rust owns context loading, configuration,
credentials, provider networking, routing, cancellation, timeout enforcement, and error
classification.

This design implements only persistent context packs and manual text generation. Capture,
audio, history, rolling summaries, fallback routing, settings UI, and OS-backed credential
storage remain outside this change.

## Provider approach

Three approaches were considered:

1. A direct OpenRouter adapter behind a provider-independent text-generation port.
2. A configurable OpenAI-compatible HTTP adapter that accepts arbitrary endpoints.
3. Multiple provider adapters in the first implementation.

The implementation uses the first approach. A direct adapter proves the router boundary
while keeping authentication headers, endpoint selection, request DTOs, and streaming
parsing confined to vendor-specific infrastructure. An arbitrary compatible endpoint would
be too close to a generic HTTP proxy and would make configuration and security validation
less precise. Multiple adapters would add testing and configuration breadth without adding
capability required by the manual-text path.

OpenRouter model identifiers remain configuration values. Documentation uses
`openrouter/free` as a zero-cost development option, while allowing an explicit model slug
when predictable behavior matters. The adapter uses OpenRouter's documented
`POST /api/v1/chat/completions` streaming API and does not expose OpenRouter DTOs outside
its module.

## Rust architecture

The Rust core gains four focused areas:

- `domain`: provider-independent identifiers, selected context, generation requests,
  streamed events, completed responses, usage, and structured failure kinds.
- `context`: strict manifest parsing, filesystem validation, deterministic selection, and
  prompt construction.
- `providers`: the text-generation port, OpenRouter adapter, and configuration-driven
  router.
- `app`: the manual-assistance service and one-active-request coordinator used by Tauri
  commands.

Commands translate typed IPC values and delegate to the application service. They do not
load files, read environment variables, construct prompts, or call OpenRouter directly.

### Provider-independent request and event model

A text-generation request contains a stable request ID, provider/model identity, selected
context, a system prompt, and the current user text. Stream events contain the same request
ID and one of these application-level outcomes:

- started;
- text delta;
- completed with optional usage metadata;
- cancelled;
- failed with a safe error code and actionable message.

Provider errors are classified as authentication, configuration, invalid request, rate
limit, timeout, transport, provider, cancellation, or malformed response. Raw provider
bodies are never returned to React or logged. Authentication and invalid-request failures
are not retried; this change adds no automatic retries or fallbacks.

### Router, timeout, and cancellation

Typed configuration selects exactly one supported provider: `openrouter`. The router owns
adapter selection and returns a configuration error for any unsupported value. This shape
allows another explicit adapter later without introducing fallback policy now.

The manual-assistance coordinator permits one active request. Starting a second request
returns a busy error. Each active provider stream has a bounded deadline after context
preparation. Cancellation or timeout drops the HTTP stream and produces exactly one
terminal event. Context loading happens before the provider-stream deadline begins. A
cancellation command must name the current request ID; stale IDs cannot cancel a newer
request.

## Configuration and secret boundary

Rust resolves non-secret configuration from process environment variables at startup:

- `SINGULARITY_LIVE_PROVIDER` must be `openrouter`;
- `SINGULARITY_LIVE_MODEL` is an OpenRouter model slug;
- `SINGULARITY_LIVE_CONTEXT_PACK` names the runtime pack directory;
- `SINGULARITY_LIVE_REQUEST_TIMEOUT_SECONDS` is optional and has a documented bounded
  default.

`OPENROUTER_API_KEY` is resolved through a Rust `SecretStore` port. The initial
`EnvironmentSecretStore` is deliberately development-only. Its secret value is not
serializable, is redacted from debug output, and never appears in command arguments,
responses, events, logs, fixtures, snapshots, browser storage, React state, or Zustand.
Persistent OS-backed credential storage remains later security work.

The frontend receives only a readiness result containing configured/unconfigured state and
safe guidance. It may receive provider and model identity because those values are
non-secret, but it never receives a credential or authorization metadata.

## Context-pack format and loading

Runtime packs live below Tauri's platform-specific application-data directory:

`<app-data>/context-packs/<configured-pack>/`

Tests inject an isolated root directory. Production loading never reads from the Git
checkout or a hard-coded user path. The repository includes a fictional example solely for
users to copy into the runtime directory.

Each pack contains `manifest.yaml` and referenced Markdown files. Schema version 1 uses
this shape:

```yaml
schema_version: 1
id: fictional-developer
name: Fictional Developer
documents:
  - id: answer-style
    title: Answer style
    path: answer-style.md
    always_include: true
    keywords: []
  - id: projects
    title: Projects
    path: projects.md
    always_include: false
    keywords: [project, architecture, rust]
```

Unknown fields are rejected. IDs and keywords are non-empty, normalized, length-bounded,
and unique where applicable. Pack paths must resolve under application data; a symlinked
pack directory is rejected. The manifest must be a bounded regular file whose canonical
path remains inside the pack. Document paths must be relative, must contain only normal
path components, and must end in `.md`. Absolute paths, parent components, platform
prefixes, symlink escapes, missing files, non-files, and canonical paths outside the pack
directory are rejected.

Limits are enforced before provider submission:

- manifest: 32 KiB;
- Markdown document: 64 KiB;
- loaded pack content: 256 KiB;
- document count: 16;
- manual user text: 16 KiB UTF-8.

These limits keep validation and provider payloads bounded while allowing substantial
human-readable context. They are documented and tested at their boundaries.

### Deterministic selection and prompt construction

Selection preserves manifest order. A document is selected when `always_include` is true or
when a normalized configured keyword appears as a complete word or phrase in normalized
manual input. Documents with no match are not sent. This intentionally small rule avoids a
speculative intent-classification subsystem while preventing unrelated context from being
included automatically.

The prompt builder uses a fixed provider-independent template. Selected documents are
rendered with stable headings and explicit boundaries, followed by a rule that context is
reference material rather than executable instruction. User text remains a separate user
message. Prompt construction has snapshot-free structural tests so content and ordering are
asserted without placing secrets or provider payloads in snapshots.

## IPC and frontend flow

The Tauri boundary adds only three operations:

- read manual-assistance readiness;
- start one validated manual request;
- cancel the active request by ID.

The build manifest and main-window capability declare only those commands. No filesystem,
networking, shell, secret, SQL, or generic dispatch permission is exposed.

At mount, the frontend subscribes to one named manual-assistance event channel and validates
every unknown payload at runtime. It ignores malformed events and events whose request ID
does not match the visible active request. The start command returns the Rust-generated
request ID; all subsequent events carry it.

The existing visual system is extended with a composer and assistant-output area. The UI
has explicit unconfigured, ready, starting, streaming, completed, cancelled, and failed
states. Send is disabled for blank input and while a request is active. Cancel is available
only during an active request. Completion, cancellation, and failure restore input controls;
focus returns to the composer after terminal events. Enter submits and Shift+Enter inserts a
newline. No history, fake conversation, capture control, or settings form is added.

## Error behavior

Configuration failures identify the missing non-secret setting or credential name without
including its value. Context errors identify the pack or relative file and validation rule,
but do not echo full context content. Provider errors expose safe categories and recovery
guidance. Malformed provider streams terminate the request as a malformed-response failure.

The frontend maps known safe error codes to actionable presentation. Unknown or malformed
errors become a generic safe failure. A terminal event is idempotent, and late deltas after a
terminal state are ignored.

## Testing strategy

Behavior is developed test-first. Rust tests cover:

- manifest success and strict rejection;
- unsupported versions, malformed YAML, missing files, and duplicate identifiers;
- Markdown loading, traversal attempts, absolute paths, and symlink escapes;
- each size/count limit;
- deterministic selection and prompt ordering;
- typed configuration and development secret lookup without secret serialization;
- router selection and unsupported providers;
- OpenRouter request translation;
- incremental SSE parsing, usage extraction, provider-stream errors, and malformed chunks;
- HTTP/status error classification with a local mock server;
- provider-stream timeout, explicit cancellation, stale cancellation, and one-active-request
  behavior;
- safe IPC request validation and event serialization.

Frontend tests mock only the Tauri boundary and cover readiness, streaming rendering,
malformed and stale events, duplicate submission, keyboard behavior, cancellation, failure
recovery, and focus restoration.

Automated tests do not require credentials or make live/paid provider calls. Final manual
validation uses the real Tauri shell when the environment and a user-supplied development
credential permit it. The roadmap entry remains incomplete if the live request cannot be
verified; documentation records that limitation honestly.

## Documentation and durable decisions

The README documents environment configuration, runtime pack locations on supported
platforms, example-pack installation, manual use, and the fact that credentials never cross
IPC. `tech.md` records the implemented modules, supported provider/model configuration,
validation evidence, limitations, and actual roadmap status.

A focused ADR records the durable choice to keep secrets, context loading, provider DTOs,
and networking behind the Rust boundary while using a development-only environment secret
source. The accepted provider-neutral ADR remains in force.
