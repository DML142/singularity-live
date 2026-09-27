# Singularity Live engineering guide

This document is the canonical product architecture, engineering policy, roadmap, and
implementation-status record. It describes both current code and deliberately planned
boundaries. A capability is not implemented unless the current-status sections say so.

## 1. Product overview

Singularity Live is a backstage creative assistant for people making live or recorded
content: programming streams, coding tutorials, gameplay videos, and dynamic video scripts.
It helps a creator reason about code and images, develop an idea while recording, and respond
to speech from the creator or a co-host. The creator sees the assistant; the audience should
not see its window in the creator's broadcast or recording.

The core interaction accepts typed text, an explicitly selected screenshot, and (when the
planned voice-input workflow is implemented) explicitly captured microphone or system audio
converted to text. The assistant returns text only. Voice input is a way to provide context,
not a request for spoken AI replies.

Keeping the assistant window out of supported screen-capture output while leaving it visible
and usable to the creator is a required product capability and release gate. It is distinct
from briefly hiding the window while this application captures its own screenshot.

### Terminology

- **Session**: one continuous assisted interaction.
- **Context pack**: versioned persistent user context stored as a YAML manifest plus
  human-readable Markdown files.
- **Transcript entry**: a time-ordered text representation of spoken or typed input.
- **Assistant response**: contextual output associated with a session request.
- **Provider**: an external service adapter that supplies a capability.
- **Model**: a configurable provider-specific model identifier.
- **Capture**: an explicit audio or image acquisition operation.
- **Attachment**: metadata describing a saved or temporary session artifact.
- **Response mode**: creator intent such as Quick, Explain, Code, or Script.

## 2. Goals

- Help creators produce accurate code, explanations, and video ideas with low perceived
  latency and clear uncertainty when the model lacks enough information.
- Combine creator-selected speech, screenshots, typed input, user context, and recent session
  state safely.
- Convert incoming speech to editable text and return assistant replies as text only.
- Keep the assistant window out of supported broadcast and recording captures without
  hiding it from the creator.
- Keep providers replaceable and model identifiers configurable.
- Treat privacy, explicit capture state, and transient media handling as product behavior.
- Remain maintainable and cross-platform, with Windows as the primary production target.
- Keep the local desktop architecture proportionate: strong boundaries without speculative
  frameworks.

## 3. Non-goals

- A cloud account platform, multi-user backend, plugin marketplace, or distributed system.
- Permanent recording of raw audio or screenshots by default.
- Spoken AI replies or audio generation.
- Interview, exam, or assessment assistance; the product is for creator workflows.
- Claiming zero hallucinations or treating generated code as verified without review.
- Direct model-provider networking or secret storage in React.
- Implementing roadmap features before their phase begins.

## 4. MVP definition

The creator MVP has two input pipelines and one required desktop-output property:

```mermaid
flowchart LR
  Speech[Explicit mic or system-audio capture] --> VAD[Local voice activity detection]
  VAD --> STT[Speech-to-text provider]
  STT --> Composer[Editable transcript in composer]
  Text[Typed text] --> Composer
  Composer -->|explicit send| Session[Session orchestrator]
  Session --> Select[Context selector]
  Select --> Router[Provider router]
  Router --> Stream[Streamed text response]
```

```mermaid
flowchart LR
  Trigger[Explicit screenshot action] --> Capture[Screen or region capture]
  Capture --> Prepare[Image preprocessing]
  Prepare --> Request[Multimodal request]
  Context[Persistent and current context] --> Request
  Request --> Result[Text response]
```

The assistant window must also be excluded from supported broadcast/recording captures while
remaining visible on the creator's desktop. Voice transcripts are reviewed or edited in the
composer and sent only after the creator acts. Audio never produces spoken assistant output.
Audio input and broadcast capture protection are not implemented in Phase 0.

## 5. Architecture

The application is a local desktop process with a presentation webview and a trusted Rust
core.

```mermaid
flowchart TD
  UI[React presentation] -->|typed Tauri IPC| Commands[Narrow commands]
  Commands --> Services[Application services]
  Services --> Domain[Provider-independent domain]
  Services --> Ports[Volatile-boundary interfaces]
  Adapters[Provider, storage, capture and OS adapters] --> Ports
  Adapters --> External[OS and external services]
```

Dependency direction is inward toward domain concepts. Commands translate the IPC boundary
and delegate; they do not contain business logic. Infrastructure adapters know their
external APIs. Domain types do not know React, individual providers, SQLite, xcap, or
WASAPI.

The current vertical slice contains application status, manual text assistance, and explicit
screen assistance. Requests pass through narrow Tauri commands to Rust services. The session
service loads selected context and sends bounded role-tagged conversation history through the
configured provider-independent router; a screenshot request adds one temporary image to
that request. One process-local session keeps successful text turns and a rolling summary
until reset or application restart. Future directories are created when code exists for
them.

## 6. Frontend responsibilities

React owns:

- semantic, accessible presentation;
- local interaction and focused UI state;
- the manual request composer, incremental rendering of backend event streams, and a new
  session action that clears the visible conversation only after Rust confirms reset;
- visible screen-assistance capability and permission states, explicit source selection and
  capture, temporary preview/crop, and separate send or discard actions;
- explicit shortcut-binding edits and chord recording, plus hotkey-capture preview/error
  presentation;
- actionable loading, empty, and safe error states.

The frontend is feature-oriented. `src/app` composes the shell, `src/features` contains
product areas, `src/components/ui` contains small reusable presentation primitives,
`src/stores` contains focused Zustand stores, and `src/lib/tauri` is the typed IPC boundary.

React does not own provider orchestration, secrets, persistence, capture, or arbitrary host
access. It must not duplicate canonical backend session state unnecessarily.

## 7. Rust responsibilities

Rust owns:

- application services and session orchestration;
- provider communication and routing;
- credential lookup behind the Rust `SecretStore` port;
- filesystem and platform paths;
- audio and screen capture;
- persistence and context loading;
- image preprocessing, versioned shortcut settings, native shortcut registration,
  cursor-to-monitor resolution, and capture-window lifecycle;
- safe error mapping and structured technical logging.

Rust exposes `get_app_status`, the manual-assistance commands, and narrow screen-assistance
commands for capability reporting, target listing, explicit capture, capture cancellation,
crop, discard, and screenshot-assisted requests. It also exposes typed
`get_shortcut_bindings` and `update_shortcut_bindings` commands; the global shortcut callback
is native-only and unavailable to the webview. `SessionService` validates text, selects
relevant static context, composes bounded system and role-tagged conversation messages,
summarizes older completed turns through the existing router when limits require it, permits
one active request, and returns typed failures. Capture, image processing, provider
networking, context loading, configuration, credential lookup, session state, cancellation,
and image deletion stay in Rust. Startup uses `expect` only for the invariant that a Tauri
application must initialize to run; runtime or user-controlled operations return typed
errors.

## 8. Security boundaries

The Tauri webview is untrusted relative to credentials and sensitive host operations.
Frontend code may request a defined action; Rust authorizes and executes it. Commands must
be narrow and typed—never a generic action dispatcher, filesystem gateway, shell wrapper,
HTTP proxy, or SQL endpoint.

Tauri capabilities grant only application commands declared in the Rust build manifest;
the main window receives application and manual-assistance permissions, the screen commands
`get_screen_capture_capabilities`, `list_screen_capture_targets`,
`start_screen_capture`, `cancel_screen_capture`, `crop_screen_capture`,
`discard_screen_capture`, and `start_screenshot_assistance`, and the shortcut settings
commands `get_shortcut_bindings` and `update_shortcut_bindings`. It also receives
`core:event:allow-listen` and `core:event:allow-unlisten` for the streaming UI, and no plugin
permissions. Provider keys never enter Vite environment variables, localStorage, Zustand,
logs, or IPC requests or responses. The current `EnvironmentSecretStore` is for local
development only; OS-backed credential storage remains future security work.

Sensitive content is excluded from logs by default. API keys, authorization headers, raw
audio, screenshots, full context packs, and full provider payloads must not be logged.

## 9. Provider architecture

Planned capability ports include speech-to-text. Text generation has direct OpenRouter and
Gemini adapters; both support provider-neutral still image parts for explicit screenshot
requests. Provider choice is an infrastructure setting, not a domain dependency.

The selected initial models are `gemini-3.5-transcribe-live` for planned transcript-only
live speech recognition and `gemini-3.8-flash` for text, image, code, and explanation
requests. Gemini text and image generation is implemented through a direct Rust adapter;
live requests with the user's key have not yet been verified in this change. Speech
recognition remains planned, not implemented. Evaluate Russian/Ukrainian technical
transcripts, screenshot interpretation, code correctness, latency, and cost before calling
either path production-ready. OpenRouter remains available as an alternative text provider.

```mermaid
flowchart LR
  Service[Application service] --> Request[Domain request]
  Request --> Router[Capability router]
  Router --> Adapter[Selected adapter]
  Adapter --> Vendor[External provider]
```

The router selects OpenRouter or Gemini from typed startup configuration and enforces the
configured model and provider-stream timeout. It supports cancellation and classifies
authentication, configuration, invalid-request, rate-limit, timeout, transport, provider,
cancellation, and malformed-response failures. It does not retry or fall back. Provider DTOs, endpoints,
authorization headers, and SSE parsing stay inside the adapter. Prompt construction is
centralized behind a context selector and prompt builder, never assembled in components.

## 10. Context architecture

Runtime context packs live under Tauri's platform-specific application data directory at
`context-packs/<pack-id>/`, never a hard-coded user path or the Git checkout. A pack uses a
strict schema-versioned YAML manifest that references human-readable Markdown files. The
repository includes a fictional and sanitized example. The loader rejects packs outside
application data, symlinked pack directories, unknown schema fields, invalid paths,
traversal and symlink escapes, missing files, and content beyond documented size limits.

Context selection is layered:

1. relevant static context;
2. rolling session summary;
3. recent transcript turns;
4. the current text, speech, or screenshot input.

The current deterministic selector preserves manifest order and includes `always_include`
documents plus documents whose configured complete keyword or phrase occurs in the text
request or the prior text intent for a screenshot. It avoids sending non-matching documents.
A Rust-owned session also includes
prior successful user and assistant messages in chronological order and automatically
compacts older turns into a rolling summary when recent history exceeds either bound. A
screenshot follow-up selects static context using the most recent text intent, includes prior
role-tagged messages, and sends the current image only in that request. Completed history and
rolling summaries retain text only. Audio, transcript, and other unsupported inputs do not
enter the session.

## 11. Session architecture

One process-local `SessionService` owns a generated session ID, `Idle`/`Active`/`Processing`
lifecycle, completed text exchanges, a rolling summary, and the active request's cancellation
token under one state lock. A screenshot request includes a temporary image in the current
provider message but stores only its text continuation prompt and completed text answer after
success. Only successfully completed text exchanges enter later model context. A failure or
cancellation leaves prior turns available, excludes any partial answer, and releases the
image. Reset is rejected while processing; otherwise it clears recent turns and summary and
returns to an idle session with a fresh ID. Reset also deletes any pending screenshot. A
process restart begins with an empty session and no retained image.

Current manual input is capped at 16 KiB. Recent history is capped at 8 exchanges and
16 KiB, the system context at 20 KiB, the rolling summary at 4 KiB, each retained assistant
answer at 16 KiB, and summary-request system plus conversation text at 64 KiB. Limits count
UTF-8 bytes. When needed, Rust stages oldest-turn summaries through the existing router and
replaces the stored summary and removed turns only after every required summary call
succeeds. Context pack loading runs on a blocking worker; provider and summary calls honor
request cancellation.

The session is deliberately volatile and has no event or chat-history persistence. It does
not introduce multiple chats, SQLite, audio inputs, transcription, or response modes. A
single explicitly submitted screenshot may accompany one provider request and is deleted
afterward; images do not enter session history. Future session event records may use stable
IDs, timestamps, source, and kind, but a durable event log is not part of the current
implementation.

## 12. Storage plan

SQLite will eventually store structured sessions, transcript entries, assistant responses,
attachment metadata, provider-request metadata, summaries, and history behind a Rust
repository layer. Screenshots and audio will use file storage plus references when users
explicitly choose persistence; large media is not stored as SQLite blobs by default.

The SQLite library and migration approach will be chosen in Phase 5 and recorded in an
ADR. No database crate or durable runtime storage exists; the implemented manual-text
session remains volatile and in memory.

## 13. Audio plan

The planned flow is `AudioSource → normalizer → local VAD → segment buffer → STT provider
→ editable transcript`. `MicrophoneSource` and `SystemAudioSource` are platform-independent
concepts. Windows system audio will use a platform adapter, likely WASAPI loopback, without
leaking Windows types into the domain. Creators can choose their microphone for their own
commentary or system audio for a co-host or game conversation.

The initial STT choice is Gemini 3.5 Transcribe Live, configured for text-only transcripts,
Russian/Ukrainian language detection, and a configurable technical-term vocabulary. The
visible voice-input shortcut starts and stops capture; the finalized transcript is inserted
into the composer for review and editing. Sending it to the text-generation path remains a
separate explicit action and can include a selected screenshot.
Live transcription sessions are limited to ten minutes and must reconnect during longer
sessions. See the [Live Transcribe guide](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe).

VAD is local and selected later using latency, CPU, packaging, portability, and accuracy
evidence. Raw audio is transient by default: capture, segment, transcribe, discard. Audio
capture, VAD, and speech-to-text are not implemented in Phase 0.

## 14. Screenshot assistance

Capture runs in Rust after an explicit user action. XCap provides one-shot monitor and window
capture on Windows, macOS, and X11. Linux Wayland uses the visible XDG ScreenCast picker and
one PipeWire frame with portal persistence disabled; unsupported backends report that state.
Region selection crops the reviewed in-memory preview. Rust bounds dimensions and encoded
size, while the UI displays a temporary preview and requires a separate send action.

One image remains in an in-memory Rust store for no more than five minutes. Send, discard,
replacement, reset, cancellation, processing errors, expiry, and process exit release its
bytes. The provider request combines it with the previous text intent and role-tagged text
context, while completed session history keeps text only. Images and preview data are never
persisted or logged. OCR is deferred until benchmarks show a benefit for indexing, local
extraction, or cost reduction.

## 15. Testing strategy

- Frontend behavior uses Vitest and React Testing Library with external IPC mocked at the
  Tauri API boundary. Tests cover readiness, event validation, streaming, stale events,
  duplicate submission, keyboard behavior, cancellation, failure recovery, and focus.
- Rust domain and application behavior uses unit and integration tests.
- The OpenRouter adapter uses a local mock HTTP server for request mapping, SSE parsing,
  still-image serialization, provider error classification, timeout, and cancellation.
  Gemini uses the same Rust port and maps images to Gemini inline data; live Gemini requests
  have not yet been verified with an API key.
  Capture tests use fake backends; session tests use a fake router for follow-up context,
  screenshot composition, bounds and summaries, relevance filtering, reset, cancellation,
  and recovery after failures. Automated tests need no API key and make no provider calls.
- Tests protect observable behavior, not private structure or prose.
- CI runs formatting, linting, type checking, tests, and builds for the current foundation.

The shell tests backend readiness and honest setup states; focused frontend tests exercise
the manual request flow, and Rust tests cover provider-independent behavior and the local
OpenRouter mock-server boundary.

## 16. Repository conventions

- Use pnpm and commit `pnpm-lock.yaml`; commit `src-tauri/Cargo.lock` for the application.
- Keep TypeScript strict and avoid `any`, suppression comments, and unsafe assertions.
- Avoid runtime `unwrap` in Rust and blocking work on async threads.
- Add a dependency only for an implemented capability.
- Use conventional semantic commit messages and feature-oriented branch names without
  `codex/`, `phase/`, or other agent/tool prefixes.
- Do not include coding-agent names or generated attribution in source, branches, or commits.
- Keep local orchestration in ignored `AGENTS.md` and `.ai/`; product decisions belong here
  or in ADRs.
- Update this document with material architecture or implementation-status changes.
- Use UTC internally for future persisted timestamps and stable IDs for entities.

The Tauri identifier is provisionally `local.singularity.live`. The `local` namespace avoids
claiming an internet domain and must be replaced with an owned release identifier before
signed distribution.

## 17. Platform strategy

Windows is the primary production target and the first target for broadcast capture
protection. Linux and macOS are secondary desktop targets. Shared domain and application
layers remain platform-independent; capture, audio, shortcuts, credential storage,
permissions, and capture protection live behind target-specific adapters.

Tauri content protection is enabled for the Windows main window. The Linux windowing backend
does not support this feature. A macOS implementation requires a separate platform decision
and validation; do not describe a build as broadcast-protected unless its capture behavior
has passed the release checks.

Unsupported functions should be surfaced through future capability reporting rather than
random runtime failure. Paths always use Tauri/platform APIs. Packaging, signing, installers,
updates, and full cross-platform validation are Phase 7 work.

## 18. Risks and open questions

- The best secure credential crate and its behavior across all targets require evaluation.
- Windows loopback capture and Linux/macOS system-audio parity may require different adapters.
- VAD accuracy, resource cost, and packaging need measurement on target hardware.
- Provider latency, availability, and pricing require routing metrics without payload logging.
- Context selection needs evaluation to avoid irrelevant or privacy-heavy prompts.
- Screenshot permission and window/region capabilities differ by display server and OS.
- Broadcast software uses different capture APIs; operating-system window protection must be
  verified with the actual supported recording and streaming paths.
- IPC type drift may justify generated bindings once the command surface becomes substantial.
- The provisional application identifier must change before distribution.

## 19. Roadmap

Each phase is complete only after its acceptance criteria are validated. Architecture
preparation does not partially complete a future phase.

### Phase 0 — Foundation

**Goal:** Establish the repository, architecture, desktop shell, security boundary,
documentation, tests, and CI without external AI functionality.

**Scope:** React/Tauri bootstrap, typed status IPC, minimal Zustand state, polished idle
shell, strict tooling, local agent safeguards, architecture record, and validation workflow.

**Out of scope:** Providers, credentials, capture, audio, persistence, real sessions,
shortcuts, overlays, tray behavior, updater, installers, and signing.

**Deliverables:** Buildable desktop foundation, frontend/Rust tests, lockfiles, README,
this guide, focused ADRs, ignored local `AGENTS.md`, and GitHub Actions CI.

**Acceptance criteria:** All checks in Section 20 pass; the Tauri shell launches with real
IPC where the environment supports GUI execution; documentation matches the code; no
tutorial remnants, secrets, fake behavior, or future dependencies exist.

**Status:** Completed

### Phase 1 — Context and provider core

**Goal:** Support safe manual text assistance through provider-independent context and model
boundaries.

**Scope:** Context-pack model/loading, typed configuration, secret-store abstraction,
provider ports and initial adapters, provider routing, streaming text, manual input, and
assistant rendering.

**Out of scope:** Screenshot and audio capture, full session intelligence, SQLite history,
and release packaging.

**Deliverables:** Sanitized context example, secure credential boundary, provider adapters,
streaming request path, manual input UI, tests, and relevant ADRs.

**Acceptance criteria:** Secrets never cross IPC; manual requests stream through the router;
context schemas validate; adapters and failure mapping are tested; docs match implementation;
a real desktop request is verified when a user-supplied development credential is available.

**Status:** Completed — implementation and automated checks are complete; the user manually
verified a real desktop OpenRouter request with text context on 2026-09-26.

### Phase 2 — Screen understanding

**Goal:** Add explicit transient screenshot assistance with current conversation context.

**Scope:** Monitor/region capture, supported window capture, preprocessing, multimodal path,
preview, and transient-lifecycle behavior.

**Out of scope:** Audio, OCR without benchmark justification, and implicit persistence.

**Deliverables:** Rust capture adapters, capability reporting, multimodal requests, preview
UI, permission/error states, and privacy tests.

**Acceptance criteria:** Users explicitly trigger capture; temporary images are deleted;
unsupported capabilities are clear; screenshot + text context produces a tested result.

**Status:** Completed — mock-backed Rust and UI tests cover explicit capture and preview,
permissions and unsupported states, bounded transient image lifetime and deletion, the
previous text request plus screenshot through the provider boundary, safe errors, and
cancellation. Full project checks passed on 2026-09-26; automated validation used neither
live provider requests nor real screen captures.

### Phase 2.5 — Global screenshot shortcuts and Binds settings

**Goal:** Let users explicitly start the existing transient screenshot flow with configurable
global shortcuts while the application is running.

**Scope:** Rust-owned shortcut registration and versioned non-secret bind settings; a Binds
view for recording, clearing, adding, removing, and saving screenshot shortcuts; monitor-under-
pointer capture on native desktops and the existing consent-driven source picker on Wayland;
always-on-top hide/capture/restore coordination; and an optional development launcher.

**Out of scope:** Audio, VAD, transcription, OCR, persistent screenshot data, stored API keys,
tray or launch-at-login behavior, and actions other than Screenshot.

**Acceptance criteria:** Global capture works while the app is active or minimized; the app is
hidden from this app's own screenshot and returns above other windows; binding edits validate
and roll back safely; preview remains temporary and is sent only by explicit user action; the
dev launcher does not echo or persist credentials; automated tests pass; and manual desktop
smoke checks pass on supported operating systems.

**Status:** In progress — implementation and automated validation are complete. Manual desktop
smoke checks for active/minimized activation, always-on-top restore, capture without
self-inclusion, and platform permission flows have not yet been recorded. Phase 3 remains
`Not started`; Phase 4 remains `In progress`.

### Phase 2.6 — Broadcast capture protection

**Goal:** Keep the assistant usable on the creator's desktop while excluding its window
contents from supported screen recordings and broadcasts.

**Scope:** Enable Tauri `contentProtected` on the Windows main window; document platform
support and verify the output with supported screen-capture sources. This protects the
assistant window in external capture. Phase 2.5's hide/capture/restore sequence remains the
separate mechanism for this app's own screenshot workflow.

**Out of scope:** Hiding the application from the creator, camera-based recording, DRM or
security guarantees, and claiming support for capture backends that have not been verified.

**Acceptance criteria:** The Windows main window requests content protection at startup; the
creator can still use the visible window; supported OBS display and window capture omit the
assistant contents on Windows 10 version 2004 or later; documentation names unsupported or
unverified platforms; no roadmap phase or release may claim broadcast protection before these
checks pass.

**Status:** Implemented in the Windows Tauri configuration; manual OBS smoke checks have not
yet been recorded.

### Phase 3 — Audio and transcription

**Goal:** Give creators low-latency voice input from their microphone or supported system
audio, including a co-host's speech, and turn it into editable text for the assistant.

**Scope:** Explicit start/stop voice-input shortcut, audio-source selection, Windows loopback
and microphone capture, local VAD, segmentation, Gemini 3.5 Transcribe Live, transcript review
and composer integration, and latency metrics. The creator sends the transcript as text after
review; generated responses stay text-only.

**Out of scope:** Permanent recording, cloud silence detection, and advanced session routing.

**Deliverables:** Platform adapters, VAD ADR/implementation, STT adapter, transcript events,
UI, privacy-safe metrics, and tests.

**Acceptance criteria:** Capture is visibly active and user-started; the selected speaker is
transcribed into editable text; the creator decides when to send it; silence stays local; raw
segments are discarded; no spoken assistant response is generated; long sessions reconnect
without losing transcript context; measured latency is documented.

**Status:** Not started

### Phase 4 — Session intelligence

**Goal:** Combine creator-session memory, context relevance, modalities, and code, explanation,
and dynamic-script response modes.

**Scope:** Complete lifecycle, recent turns, rolling summaries, intent/context routing,
combined screenshot + transcript requests through Gemini 3.8 Flash, modes, interruption, and
cancellation.

**Out of scope:** Long-term history UI and release packaging.

**Deliverables:** Session orchestrator, summary/context services, multimodal composition,
response-mode behavior, cancellation, and integration tests.

**Acceptance criteria:** One active session combines supported inputs; irrelevant static
context is excluded; cancellation is reliable; lifecycle/error states are explicit.

**Status:** In progress — the manual-text session has bounded role-tagged history, automatic
static-context selection, provider-mediated rolling summaries, lifecycle reset, and
cancellation/error coverage. The Phase 2 screenshot path now composes one temporary image
with prior text intent and context. Transcript composition, response modes, broader
multimodal session behavior, and other unsupported inputs remain unimplemented.

### Phase 5 — Persistence and context management

**Goal:** Persist structured history and let users manage context and retention locally.

**Scope:** SQLite, migrations, history, context management/import/export, preferences,
retention, and explicit attachment persistence.

**Out of scope:** Cloud synchronization and implicit raw-media retention.

**Deliverables:** Storage ADR, repository adapters, migrations, management UI, settings,
retention enforcement, and tests.

**Acceptance criteria:** Migrations are repeatable; React cannot issue SQL; platform data
paths are used; export/import is validated; retention behavior is explicit and tested.

**Status:** Not started

### Phase 6 — Reliability and security

**Goal:** Harden failures, credentials, routing, recovery, and local observability.

**Scope:** Retry/timeout/fallback policy, rate limits, credential implementation, log
scrubbing, recovery, offline-safe behavior, and diagnostics.

**Out of scope:** New core product modalities and release signing.

**Deliverables:** Secure store adapter, policy router, scrubbed structured logging,
diagnostics, recovery paths, and fault-injection tests.

**Acceptance criteria:** Failure classes drive correct retry/fallback behavior; credentials
are OS-backed; sensitive payloads never log; offline startup is safe; recovery is tested.

**Status:** Not started

### Phase 7 — Desktop polish and release

**Goal:** Produce accessible, performant, signed release artifacts for supported platforms.

**Scope:** Shortcuts, compact behavior, settings polish, accessibility, performance,
production icons, installers, release automation, update/signing strategy, and platform
validation.

**Out of scope:** Unrelated cloud services.

**Deliverables:** Release UX, signed installers where configured, measured performance,
cross-platform reports, and release documentation.

**Acceptance criteria:** Supported installers launch and update safely; accessibility checks
pass; broadcast capture protection passes its supported-platform checks before release;
shortcuts and compact mode are ordinary visible UX; measurements are reproducible.

**Status:** Not started

## 20. Current implementation status

### Implemented

- pnpm workspace with pinned package manager and committed lockfile.
- Strict React/TypeScript/Vite/Tailwind frontend with focused Zustand stores.
- Accessible desktop shell with a manual text composer, streaming response, cancellation,
  readiness guidance, safe failures, keyboard handling, and focus restoration.
- Responsive frontend conversation view with Markdown/code formatting and an ephemeral
  transcript; completed turns are sent as bounded role-tagged context for follow-up requests.
- A single in-memory Rust session with automatic current-input relevance filtering, bounded
  recent exchanges and system context, rolling summaries through the existing router, and a
  reset lifecycle that rejects active requests.
- Accessible **New session** action that waits for Rust reset confirmation before clearing
  visible turns and preserves the conversation with a safe error if reset fails.
- Explicit screen assistance with capability and permission states, monitor/window target
  selection, validated region crop, in-memory preview, separate send/discard actions, and
  cleanup on expiry, reset, unmount, errors, and cancellation.
- Rust-only platform capture adapters, bounded image preparation, a five-minute transient
  image store, request-scoped image parts through the provider-neutral router, and OpenRouter
  and Gemini image mappings that preserve text-only request compatibility.
- Versioned non-secret shortcut bindings, transactional native shortcut registration and
  Wayland portal integration, monitor-under-pointer capture coordination, an always-on-top
  window lifecycle, and the Settings → Binds view with transient hotkey previews.
- Windows-specific Tauri `contentProtected` configuration for the main window. Its behavior
  with real OBS display and window capture still needs manual smoke validation.
- A development-only interactive/argument launcher with numbered workflow, model, and
  installed context-pack choices that passes credentials only to the spawned process
  without echoing or persisting them.
- Typed clients for application status and manual-assistance IPC; unknown event payloads are
  validated at runtime and stale request IDs are ignored.
- Rust application-status service and `SessionService` with one active request.
- Versioned context-pack validation, safe Markdown loading, deterministic selection, and
  bounded prompt construction.
- Typed OpenRouter/Gemini configuration, Rust-only environment secret lookup for
  `OPENROUTER_API_KEY` and `GEMINI_API_KEY`, provider-independent text-generation ports,
  router, streaming adapters, timeout, cancellation, and safe failure classification.
- Explicit permissions for the implemented application and screen-assistance commands and
  event listening and cleanup, no Tauri plugin permissions, and a production content security
  policy without `unsafe-inline`.
- Fictional context-pack example, OpenRouter and Gemini setup instructions, and focused
  provider and credential-boundary ADRs.
- Frontend behavior test, formatting, lint, type checking, build scripts, and CI.
- Rust formatting, Clippy, test, and check scripts.
- Public README, this engineering guide, and focused ADRs.
- Ignored local `AGENTS.md`, `.ai/`, and common agent metadata.

### Architecturally planned, not implemented

Additional provider adapters, OS-backed credential storage, multiple chats, persisted
session history, audio capture, VAD, transcription, voice-input controls, OCR, SQLite,
history management, tray, compact mode, updater, signing, and production packaging.

### Phase 0 validation record

Phase 0 was validated on 2026-09-21. The native process and real IPC response were inspected
through WebKitGTK's development inspector; no capture or provider permissions were granted.

| Check                                          | Result                                                      |
| ---------------------------------------------- | ----------------------------------------------------------- |
| Frontend format, lint, typecheck, tests, build | Passed on Node 24.21.0 and pnpm 12.5.1                      |
| Rust fmt, Clippy, tests, check                 | Passed on Rust 1.98.0                                       |
| Tauri GUI launch and real IPC                  | Passed; native webview reported `Backend ready` at 1080×720 |
| Ignore rules, metadata, tutorial, secret scan  | Passed repository review                                    |
| README command and architecture accuracy       | Passed repository review                                    |

### Context and provider validation record

Automated tests use local fixtures and mock HTTP responses; they do not need
`OPENROUTER_API_KEY` and do not make live or paid provider calls. The user manually verified
the real desktop OpenRouter text-context flow on 2026-09-26; this satisfies Phase 1's final
acceptance criterion.

| Check                                                                                 | Result                                                                                  |
| ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 pnpm check` | Passed on 2026-09-25: 11 frontend tests, 47 Rust tests, lint, types, builds, and checks |
| Tauri development startup                                                             | Built and started the native binary with provider configuration explicitly unset        |
| Live OpenRouter request                                                               | User manually verified real desktop text-context assistance on 2026-09-26               |

Direct Gemini text and screenshot generation was added on 2026-09-27. Formatting, lint,
TypeScript checks, frontend build, and Rust compilation passed. Rust Clippy reported only
the existing platform-shortcut dead-code warnings. The new Gemini adapter has not yet been
verified with a live API key; automated tests were not run for this change.

### Session intelligence validation record

This stage adds one volatile manual-text session. It does not complete Phase 4: broader
multimodal composition, transcript inputs, and response modes remain planned. All provider
tests use local mocks or a fake router; no live OpenRouter call is required.

| Check                               | Result                                                                                                  |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Rust backend tests                  | Passed: 66 tests, including follow-up, bounds, compaction, relevance, reset, cancellation, and recovery |
| Frontend reset and transcript tests | Passed: 14 targeted tests covering reset IPC, success ordering, busy state, and failure preservation    |

### Global shortcut and developer launcher validation record

Automated checks use mock shortcut registrars, capture backends, portal sessions, and local
provider fixtures. They do not capture the desktop or make live provider requests. The
manual desktop smoke criteria remain outstanding.

| Check                                          | Result                                                                                                                                           |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Frontend format, lint, typecheck, tests, build | Passed on 2026-09-27: 44 tests across 10 files                                                                                                   |
| Rust format, Clippy, tests, and check          | Passed on 2026-09-27: 119 tests and `cargo check`                                                                                                |
| Tauri development startup                      | Started on Linux Wayland with provider configuration and `OPENROUTER_API_KEY` unset                                                              |
| Desktop smoke on supported operating systems   | Attempted on Ubuntu 24.04.4 / GNOME 46 Wayland: GlobalShortcuts portal interface is absent, so binds cannot register; Xorg smoke remains pending |
| Full project `pnpm check`                      | Passed on 2026-09-27: 44 frontend tests, 119 Rust tests, formatting, lint, TypeScript, build, Clippy, and Cargo check                            |

### Screen assistance validation record

All provider-facing tests use local mock HTTP responses or a fake Rust router. The run did
not send a live provider request or capture a real desktop image.

| Check                                         | Result                                                                                                                                                                     |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Frontend tests                                | Passed: 25 tests across 4 files                                                                                                                                            |
| Rust tests                                    | Passed: 94 tests, including capability/permission states, expiry/deletion, screenshot plus prior text context, provider errors/cancellation, and PipeWire frame conversion |
| Full project `pnpm check`                     | Passed on 2026-09-26: formatting, ESLint, TypeScript, frontend tests, production build, Rust fmt/Clippy/tests, and Cargo check                                             |
| Live provider request and real screen capture | Not performed; automated acceptance used mocks and in-memory pixel fixtures                                                                                                |

### Toolchain and tested versions

The project requires Node.js 24 LTS, pnpm 12.5.1, and Rust 1.98.0. Exact JavaScript and
Rust dependency resolutions are authoritative in their lockfiles. Bootstrap selected these
current stable compatible direct versions:

| Tool or library                | Version         |
| ------------------------------ | --------------- |
| React / React DOM              | 19.3.0          |
| TypeScript                     | 6.0.3           |
| Vite                           | 8.3.0           |
| Tauri JavaScript API / CLI     | 2.11.1 / 2.11.5 |
| Tauri Rust crate / build crate | 2.11.6 / 2.6.3  |
| Tailwind CSS                   | 4.3.3           |
| Zustand                        | 5.0.15          |
| Vitest / React Testing Library | 5.0.1 / 16.3.3  |
| Rust                           | 1.98.0          |

## 21. Decision log and ADR references

- [ADR 0001: Tauri desktop architecture](docs/adr/0001-tauri-desktop-architecture.md)
- [ADR 0002: Provider-neutral application boundary](docs/adr/0002-provider-neutral-application-boundary.md)
- [ADR 0003: OpenRouter and the manual-assistance trust boundary](docs/adr/0003-openrouter-manual-assistance-boundary.md)
- [ADR 0004: Ephemeral session context and Rust-owned lifecycle](docs/adr/0004-ephemeral-session-context.md)
- [ADR 0005: Ephemeral screen assistance and provider-neutral image requests](docs/adr/0005-ephemeral-screen-assistance.md)
- [ADR 0006: Global screenshot shortcuts and Rust-owned capture lifecycle](docs/adr/0006-global-screenshot-shortcuts.md)
- [ADR 0007: Direct Gemini generation for text and screenshots](docs/adr/0007-gemini-generation.md)
- [ADR 0008: Windows broadcast capture protection](docs/adr/0008-windows-broadcast-capture-protection.md)

Future ADRs are created only for decisions that need durable context, including the secret
store, SQLite/migration strategy, VAD implementation, and materially changed platform
boundaries. Routine implementation details do not require ADRs.
