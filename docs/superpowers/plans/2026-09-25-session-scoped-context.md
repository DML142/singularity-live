# Session-Scoped Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the single active session send bounded, relevant, successful prior exchanges to the configured provider and reset them on request.

**Architecture:** Add typed user/assistant messages to the provider-neutral request and map them in the OpenRouter adapter. A Rust `SessionService` owns volatile lifecycle and recent context, uses the configured router to compact old turns into a bounded summary, and exposes one reset command; React only renders the conversation and reset state.

**Tech Stack:** Rust 2024, Tokio, Tauri 2, reqwest/OpenRouter adapter, React 19, strict TypeScript, Zustand, Vitest, Rust integration tests with a mock router.

**Spec:** [`docs/superpowers/specs/2026-09-25-session-scoped-context-design.md`](../specs/2026-09-25-session-scoped-context-design.md)

## Global Constraints

- One process-local session only; restart or reset clears it, and no session data is persisted.
- Current manual input stays at 16 KiB; recent history stays at 8 exchanges and 16 KiB; system context stays at 20 KiB; rolling summary stays at 4 KiB; combined request text stays at 64 KiB.
- Retain at most 16 KiB of each assistant response for future model context while continuing to stream the full response to the UI.
- Context limits count UTF-8 bytes; truncation stops at a character boundary.
- Summaries use the configured Rust router only when recent history exceeds a bound; staged compaction commits only after every required summary call succeeds.
- Routine tests use a mock provider and never require an OpenRouter key or make a live request.
- Provider calls, context selection, session lifecycle, cancellation, and secrets remain in Rust; React owns presentation and local interaction only.
- Do not add screenshots, multimodal requests, audio, transcription, multiple chats, durable history, SQLite, or response modes.
- Keep TypeScript strict, add no dependency for tokenization, and never log prompts, context text, credentials, or provider payloads.
- Keep `AGENTS.md` and other local agent metadata untracked; use feature-oriented branch names and semantic commit subjects without roadmap phase numbers.

## Review Focus

- Multi-byte Unicode at input, response-retention, document, and summary byte boundaries must remain valid UTF-8; Task 2 tests each truncation helper.
- Oversized static context must not exceed the system-prompt cap or hide the current request; Task 2 tests the capped prompt and current-input preservation.
- Failure or cancellation in a later summary batch must not partially replace the stored summary or remove turns; Task 3 tests transactional compaction failure.
- Reset racing with a request must return `Busy`, and a reset failure must leave visible turns intact; Tasks 3 and 5 test both sides.
- Cancelled or failed partial assistant text must remain presentation-only and never appear in the next provider request; Task 3 tests cancellation and provider failure followed by a new request.

---

### Task 1: Carry role-tagged history through the provider boundary

**Files:**

- Modify: `src-tauri/src/domain/generation.rs`
- Modify: `src-tauri/src/providers/openrouter.rs`
- Test: `src-tauri/tests/openrouter_adapter.rs`

**Interfaces:**

- Produces `ConversationRole::{User, Assistant}` and `ConversationMessage { role, content }` in `domain/generation.rs`.
- Changes `TextGenerationRequest` to carry `messages: Vec<ConversationMessage>` after `system_prompt`; the vector contains prior completed turns followed by the current user input.
- `OpenRouterRequest.messages` becomes a vector containing one system message followed by every conversation message in order.

- [x] **Step 1: Add a failing adapter mapping test**

Add this test to `src-tauri/tests/openrouter_adapter.rs`, using the existing mock HTTP server and request helper:

```rust
#[tokio::test]
async fn serializes_conversation_history_in_role_order() {
    let (endpoint, captured_request) = start_server(MockResponse {
        status: 200,
        body_chunks: vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        ],
        delay_before_response: Duration::ZERO,
    })
    .await;
    let adapter = OpenRouterAdapter::with_test_endpoint(
        reqwest::Client::new(),
        endpoint,
        Duration::from_secs(2),
    );
    let mut generation_request = request();
    generation_request.messages = vec![
        ConversationMessage::user("What does this code do?"),
        ConversationMessage::assistant("Send me the code and I will explain it."),
        ConversationMessage::user("Explain this function."),
    ];

    adapter
        .stream(
            &generation_request,
            &secret(),
            CancellationToken::new(),
            &RecordingSink::default(),
        )
        .await
        .expect("mock stream succeeds");

    let raw_request = captured_request.await.expect("request was captured");
    let body = raw_request.split("\r\n\r\n").nth(1).expect("request body");
    let json: serde_json::Value = serde_json::from_str(body).expect("request JSON");
    assert_eq!(json["messages"][0]["role"], "system");
    assert_eq!(json["messages"][1]["role"], "user");
    assert_eq!(json["messages"][1]["content"], "What does this code do?");
    assert_eq!(json["messages"][2]["role"], "assistant");
    assert_eq!(json["messages"][2]["content"], "Send me the code and I will explain it.");
    assert_eq!(json["messages"][3]["role"], "user");
    assert_eq!(json["messages"][3]["content"], "Explain this function.");
}
```

- [x] **Step 2: Run the test and confirm the current two-message request fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test openrouter_adapter serializes_conversation_history_in_role_order`

Expected: FAIL because `TextGenerationRequest` cannot carry role-tagged history and OpenRouter still serializes exactly two messages.

- [x] **Step 3: Add domain message types and variable-length adapter mapping**

Add provider-independent types:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
}

impl ConversationMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: ConversationRole::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: ConversationRole::Assistant, content: content.into() }
    }
}
```

Change `OpenRouterRequest.messages` from `[OpenRouterMessage; 2]` to `Vec<OpenRouterMessage>`. Build it with the existing system prompt first, then map each request message to `"user"` or `"assistant"` while preserving order. Update `request()` in the adapter tests to include a single current user message.

- [x] **Step 4: Run the adapter test target**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test openrouter_adapter`

Expected: PASS, including existing SSE, cancellation, timeout, and failure-classification tests.

- [x] **Step 5: Commit the provider boundary change**

```bash
git add src-tauri/src/domain/generation.rs src-tauri/src/providers/openrouter.rs src-tauri/tests/openrouter_adapter.rs
git commit -m "feat: send role-tagged conversation messages"
```

### Task 2: Add bounded session history and prompt composition

**Files:**

- Create: `src-tauri/src/context/session.rs`
- Modify: `src-tauri/src/context/mod.rs`
- Modify: `src-tauri/src/context/prompt.rs`
- Test: unit tests in `src-tauri/src/context/session.rs`

**Interfaces:**

- Produces `SessionTurn { user_text, assistant_text }` and `SessionHistory` with private `rolling_summary` and `recent_turns` fields.
- `SessionTurn::new(user_text: impl Into<String>, assistant_text: impl Into<String>)` and `SessionHistory::from_turns(Vec<SessionTurn>)` construct testable history values.
- `SessionHistory::messages_with_current(&self, current_user_text: &str) -> Vec<ConversationMessage>` returns chronological turns plus the current user message.
- `SessionHistory::next_summary_batch(&self) -> Option<SummaryBatch>` selects the smallest oldest complete prefix needed to satisfy the recent-history bounds, capped to the largest prefix fitting the 64 KiB summary-request text budget; `apply_summary_batch(&mut self, batch: SummaryBatch, summary: String)` stages it in a working copy.
- Produces `build_session_system_prompt(context: &SelectedContext, summary: Option<&str>) -> String` with the 20 KiB cap.

- [x] **Step 1: Add failing tests for role order, compaction, and UTF-8-safe caps**

Add unit tests with these assertions:

```rust
#[test]
fn messages_with_current_keeps_turns_in_chronological_role_order() {
    let history = SessionHistory::from_turns(vec![
        SessionTurn::new("first question", "first answer"),
        SessionTurn::new("second question", "second answer"),
    ]);

    assert_eq!(
        history.messages_with_current("third question"),
        vec![
            ConversationMessage::user("first question"),
            ConversationMessage::assistant("first answer"),
            ConversationMessage::user("second question"),
            ConversationMessage::assistant("second answer"),
            ConversationMessage::user("third question"),
        ],
    );
}

#[test]
fn summary_batch_reduces_history_below_both_recent_limits() {
    let turns = (0..9)
        .map(|index| SessionTurn::new(format!("Question {index}"), format!("Answer {index}")))
        .collect();
    let mut history = SessionHistory::from_turns(turns);
    let batch = history.next_summary_batch().expect("ninth turn requires compaction");
    history.apply_summary_batch(batch, "Earlier intent: explain the code.".to_owned());

    assert_eq!(history.recent_turn_count(), 8);
    assert!(history.recent_turn_bytes() <= MAX_RECENT_HISTORY_BYTES);
    assert!(history.rolling_summary().expect("summary exists").contains("Earlier intent"));
}
```

Also test oversized turns, a long selected document, a summary containing multi-byte text, and a long assistant response. Assert each retained string is valid UTF-8, each limit is respected, and the current user message is always last and unchanged.

- [x] **Step 2: Run the context unit tests and confirm the new module is missing**

Run: `cargo test --manifest-path src-tauri/Cargo.toml context::session`

Expected: FAIL because `context::session` and its bounded history types do not exist.

- [x] **Step 3: Implement bounded history and system-prompt assembly**

Add constants for 8 recent exchanges, 16 KiB recent history, 20 KiB system context, 4 KiB summary, 16 KiB retained assistant response, and 64 KiB summary-request text. Use `VecDeque<SessionTurn>` internally. Keep the newest suffix raw; select old complete turns for summary until both raw-history bounds hold. Build batches with the previous summary plus the largest oldest prefix fitting the summary-request limit. Cap summary and retained assistant content at a UTF-8 character boundary. Compose safety instructions and summary before manifest-ordered selected documents; truncate the last selected document at a character boundary when the system-context budget is reached.

- [x] **Step 4: Run context and existing context-pack tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml context::session && cargo test --manifest-path src-tauri/Cargo.toml --test context_pack`

Expected: PASS; existing `always_include`, keyword matching, and manifest-order behavior remains intact.

- [x] **Step 5: Commit the bounded context primitives**

```bash
git add src-tauri/src/context/session.rs src-tauri/src/context/mod.rs src-tauri/src/context/prompt.rs
git commit -m "feat: bound session context and summaries"
```

### Task 3: Make Rust own session lifecycle and transactional summarization

**Files:**

- Create: `src-tauri/src/app/session.rs`
- Modify: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/manual_assistance.rs`
- Test: `src-tauri/tests/manual_assistance.rs`

**Interfaces:**

- Produces `SessionService::configured(...)`, `SessionService::unconfigured(...)`, `SessionService::start(...)`, `SessionService::cancel(...)`, and `SessionService::reset() -> Result<(), ManualAssistanceError>`.
- `SessionService` owns `SessionLifecycle::{Idle, Active, Processing}`, one `SessionHistory`, and at most one active request token under one mutex so reset and request reservation are mutually exclusive.
- A summary call uses the same `TextGenerationRouter` and a private collecting sink; the answer call uses a forwarding-and-collecting sink that streams all deltas while retaining at most 16 KiB for context.
- The service stages all summary batches in a clone and replaces stored history only after every batch succeeds.

- [x] **Step 1: Add failing service tests for follow-up context and reset isolation**

Extend the fake router in `src-tauri/tests/manual_assistance.rs` to record each `TextGenerationRequest` and return scripted stream text. Add:

```rust
async fn complete_request(service: &Arc<SessionService>, text: &str) {
    let (sink, mut receiver) = channel_sink();
    service
        .start(text.to_owned(), sink)
        .await
        .expect("request starts");

    loop {
        match next_event(&mut receiver).await {
            StreamEvent::Completed(_) => return,
            StreamEvent::Failed { error, .. } => panic!("request failed: {error}"),
            StreamEvent::Cancelled { .. } => panic!("request was cancelled"),
            StreamEvent::Started { .. } | StreamEvent::TextDelta { .. } => {}
        }
    }
}

#[tokio::test]
async fn follow_up_request_includes_only_prior_completed_turns() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::new(RouterBehavior::Success));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, "What does this code do?").await;
    complete_request(&service, "Here is the function.").await;

    let requests = requests.lock().expect("captured requests");
    assert_eq!(
        requests[1].messages,
        vec![
            ConversationMessage::user("What does this code do?"),
            ConversationMessage::assistant("First answer"),
            ConversationMessage::user("Here is the function."),
        ],
    );
}

#[tokio::test]
async fn reset_removes_prior_turns_and_summary_from_the_next_request() {
    let pack = PackFixture::new();
    let router = Arc::new(FakeRouter::new(RouterBehavior::Success));
    let requests = Arc::clone(&router.captured);
    let service = service(&pack, router);
    complete_request(&service, "Old session question").await;
    service.reset().expect("idle session resets");
    complete_request(&service, "New session question").await;

    assert_eq!(
        requests.lock().expect("captured requests")[1].messages,
        vec![ConversationMessage::user("New session question")],
    );
    assert!(!requests.lock().expect("captured requests")[1]
        .system_prompt
        .contains("Old session question"));
}
```

- [x] **Step 2: Run the follow-up test and confirm the current stateless service fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test manual_assistance follow_up_request_includes_only_prior_completed_turns`

Expected: FAIL because the current service submits a single `user_text` and stores no completed session turns.

- [x] **Step 3: Implement the session coordinator and summary transaction**

Move the configured runtime, readiness, request reservation, and cancellation ownership into `SessionService`. Keep readiness and provider error mapping safe. Load/select context on `spawn_blocking`, check cancellation after the blocking operation, compact an over-limit cloned `SessionHistory` through the router, and emit summary failures as safe terminal events. Build the answer request from staged history. Commit the staged summary and append the user/assistant exchange only on a successful answer; do not append on cancellation or error. `reset()` returns `Busy` while active; otherwise it clears history and summary and returns the lifecycle to `Idle` with a new session ID.

Add tests that exceed the turn limit and assert the summary request retains the old user goal, the answer request retains the summary and latest eight exchanges, and context bytes remain under the caps. Add failure tests for an empty summary, a provider error in a later batch, cancellation during summary, cancellation after partial answer, and provider failure after partial answer. Assert failed compaction leaves prior history unchanged and no partial exchange appears in the next request.

- [x] **Step 4: Run all manual-assistance/session tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test manual_assistance`

Expected: PASS for readiness, validation, context relevance, streaming, one-active-request behavior, follow-up history, summary compaction, reset, cancellation, and recovery after failures.

- [x] **Step 5: Commit the Rust session service**

```bash
git add src-tauri/src/app/session.rs src-tauri/src/app/mod.rs src-tauri/src/lib.rs src-tauri/src/commands/manual_assistance.rs src-tauri/tests/manual_assistance.rs
git commit -m "feat: manage one in-memory assistance session"
```

### Task 4: Expose session reset through narrow Tauri IPC

**Files:**

- Modify: `src-tauri/src/commands/manual_assistance.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/main-window.json`
- Test: `src-tauri/tests/command_permissions.rs`
- Generate: `src-tauri/permissions/autogenerated/reset_session.toml`

**Interfaces:**

- Produces Tauri command `reset_session`, returning `Result<(), CommandError>` and mapping a busy service to error code `"busy"`.
- Registers `reset_session` in both `tauri::generate_handler!` and `tauri_build::AppManifest::commands`.
- Grants only `allow-reset-session` and `core:event:allow-listen` to the main window in addition to existing command permissions.

- [x] **Step 1: Add a failing Tauri permission test**

Create `src-tauri/tests/command_permissions.rs`:

```rust
use std::{fs, path::PathBuf};

#[test]
fn main_window_can_listen_and_reset_session() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/main-window.json"))
            .expect("main-window capability is valid JSON");
    let permissions = capability["permissions"].as_array().expect("permission list");

    assert!(permissions.iter().any(|value| value == "core:event:allow-listen"));
    assert!(permissions.iter().any(|value| value == "allow-reset-session"));

    let permission_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("permissions/autogenerated/reset_session.toml");
    let reset_permission = fs::read_to_string(permission_path).expect("reset permission exists");
    assert!(reset_permission.contains("identifier = \"allow-reset-session\""));
}
```

- [x] **Step 2: Run the permission test and confirm the baseline lacks reset**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test command_permissions`

Expected: FAIL because merged `main` lacks `core:event:allow-listen`, `allow-reset-session`, and the generated reset permission.

- [x] **Step 3: Add the typed command, registration, and capability entries**

Implement:

```rust
#[tauri::command]
pub async fn reset_session(
    service: State<'_, Arc<SessionService>>,
) -> Result<(), CommandError> {
    service.reset().map_err(CommandError::from)
}
```

Register it in `src-tauri/src/lib.rs` and `src-tauri/build.rs`. Add `"allow-reset-session"` and `"core:event:allow-listen"` to `main-window.json`. The event-listen permission carries forward the existing local fix required by the merged chat UI; do not copy any other local changes.

- [x] **Step 4: Regenerate and verify Tauri permissions**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`

Expected: PASS and produce `src-tauri/permissions/autogenerated/reset_session.toml` with identifier `allow-reset-session`; the main-window capability includes event listening and reset permission, with no plugin permissions.

- [x] **Step 5: Commit the Tauri reset boundary**

```bash
git add src-tauri/src/commands/manual_assistance.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/capabilities/main-window.json src-tauri/permissions/autogenerated/reset_session.toml
git commit -m "feat: expose session reset command"
```

### Task 5: Add the single-session reset interaction to React

**Files:**

- Modify: `src/lib/tauri/manual-assistance-client.ts`
- Test: `src/lib/tauri/manual-assistance-client.test.ts`
- Modify: `src/stores/manual-assistance-store.ts`
- Modify: `src/features/assistant/AssistantPanel.tsx`
- Test: `src/features/assistant/AssistantPanel.test.tsx`
- Modify: `src/app/styles/global.css`

**Interfaces:**

- Produces `resetManualAssistanceSession(): Promise<void>` in the typed IPC client, invoking `reset_session` with no arguments.
- Adds store action `resetSession(): Promise<void>` and `resetPending` / `resetError` presentation state. It clears turns, active turn, request state, and the next turn counter only after Rust reset succeeds.
- Adds an accessible **New session** button disabled during request or reset; a reset failure preserves visible turns and shows a safe error.

- [x] **Step 1: Add failing IPC and UI reset tests**

Add this IPC client test:

```ts
it("invokes the Rust session reset command", async () => {
  await resetManualAssistanceSession();
  expect(invoke).toHaveBeenCalledWith("reset_session");
});
```

Add panel tests that click **New session**, verify the Rust call completes before the old turn disappears, verify the button is disabled while a request is active, and reject the reset promise to verify the old turn remains visible with a safe error.

- [x] **Step 2: Run targeted frontend tests and confirm they fail**

Run: `pnpm vitest run src/lib/tauri/manual-assistance-client.test.ts src/features/assistant/AssistantPanel.test.tsx`

Expected: FAIL because the client, store action, and button do not exist.

- [x] **Step 3: Implement the reset client, store action, and button**

Call `resetManualAssistanceSession()` only when request phase is neither `starting` nor `streaming`. Keep current turn data while the command runs. On success, clear the visible transcript and reset the store to idle; on failure, preserve turns and set a safe `resetError`. Render the button and error inside the assistant panel, and add compact styles in `global.css` without changing the responsive chat layout.

- [x] **Step 4: Run targeted frontend tests and static checks**

Run: `pnpm vitest run src/lib/tauri/manual-assistance-client.test.ts src/features/assistant/AssistantPanel.test.tsx && pnpm typecheck && pnpm lint`

Expected: PASS with strict TypeScript and no new lint warnings.

- [x] **Step 5: Commit the React reset interaction**

```bash
git add src/lib/tauri/manual-assistance-client.ts src/lib/tauri/manual-assistance-client.test.ts src/stores/manual-assistance-store.ts src/features/assistant/AssistantPanel.tsx src/features/assistant/AssistantPanel.test.tsx src/app/styles/global.css
git commit -m "feat: add a new session action"
```

### Task 6: Record the architecture and verify the complete stage

**Files:**

- Create: `docs/adr/0004-ephemeral-session-context.md`
- Modify: `docs/adr/README.md`
- Include: `docs/superpowers/plans/2026-09-25-session-scoped-context.md`
- Modify: `tech.md`

**Interfaces:**

- ADR 0004 records the accepted one-session, volatile Rust ownership, role-tagged provider context, bounded provider-mediated summaries, and reset contract.
- `tech.md` describes the implemented manual-text session and retains Phase 4 as in progress because multimodal composition and response modes remain unimplemented.

- [x] **Step 1: Add ADR 0004 and update the ADR index**

Use the structure in `docs/adr/template.md`: status/date, context, decision, alternatives, and consequences. State that session state is process-local, no persistent history is introduced, and summaries use the existing Rust router only after a context bound is exceeded.

- [x] **Step 2: Update implementation status and the Tauri permission record in `tech.md`**

Document the one active session, role-tagged recent turns, summary/context limits, reset lifecycle, and mock-provider tests. Record `core:event:allow-listen` and `allow-reset-session` in the main-window permission list. Keep Phase 4 in progress and list image, audio, response modes, and all Phase 5 persistence work as remaining scope.

- [x] **Step 3: Review the diff for scope, secrets, and generated metadata**

Run: `git diff --check && git status --short`

Expected: only session-service, provider-message, reset IPC/UI, approved plan/spec, ADR, and `tech.md` changes appear; no credentials, `.ai/`, `AGENTS.md`, or unrelated local edits appear.

- [x] **Step 4: Run all project checks**

Run: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 pnpm check`

Expected: formatting, lint, TypeScript, frontend tests/build, Rust formatting, Clippy, Rust tests, and Cargo check all pass; provider tests use mocks and make no live OpenRouter calls.

- [x] **Step 5: Commit the architecture record and accepted status update**

```bash
git add docs/adr/0004-ephemeral-session-context.md docs/adr/README.md docs/superpowers/plans/2026-09-25-session-scoped-context.md tech.md
git commit -m "docs: record ephemeral session context"
```

After all acceptance tests pass, review the full branch diff for unrelated changes. Do not create a PR until the feature is coherent and the acceptance checks pass.
