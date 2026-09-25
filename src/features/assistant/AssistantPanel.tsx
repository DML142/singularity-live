import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type SubmitEvent,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { WorkspacePanel } from "../../components/ui/WorkspacePanel";
import { useManualAssistanceStore } from "../../stores/manual-assistance-store";

export function AssistantPanel() {
  const readiness = useManualAssistanceStore((state) => state.readiness);
  const phase = useManualAssistanceStore((state) => state.phase);
  const turns = useManualAssistanceStore((state) => state.turns);
  const cancelPending = useManualAssistanceStore((state) => state.cancelPending);
  const resetPending = useManualAssistanceStore((state) => state.resetPending);
  const resetError = useManualAssistanceStore((state) => state.resetError);
  const initialize = useManualAssistanceStore((state) => state.initialize);
  const start = useManualAssistanceStore((state) => state.start);
  const cancel = useManualAssistanceStore((state) => state.cancel);
  const resetSession = useManualAssistanceStore((state) => state.resetSession);
  const [text, setText] = useState("");
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const conversationRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const previousPhase = useRef(phase);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void initialize().then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [initialize]);

  useEffect(() => {
    const wasActive =
      previousPhase.current === "starting" || previousPhase.current === "streaming";
    const isTerminal =
      phase === "completed" || phase === "cancelled" || phase === "failed";
    if (wasActive && isTerminal) {
      composerRef.current?.focus();
    }
    previousPhase.current = phase;
  }, [phase]);

  useEffect(() => {
    const conversation = conversationRef.current;
    if (conversation !== null && stickToBottom.current) {
      conversation.scrollTop = conversation.scrollHeight;
    }
  }, [turns]);

  const submitPrompt = (prompt: string) => {
    if (prompt.trim().length > 0) {
      stickToBottom.current = true;
      setText("");
      if (composerRef.current !== null) {
        composerRef.current.style.height = "56px";
      }
      void start(prompt);
    }
  };

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    submitPrompt(text);
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submitPrompt(text);
    }
  };

  const busy = phase === "starting" || phase === "streaming";
  const detail =
    readiness.phase === "ready"
      ? readiness.value.model
      : readiness.phase === "loading"
        ? "Checking configuration"
        : readiness.phase === "unavailable"
          ? "Unavailable"
          : "Needs setup";

  return (
    <WorkspacePanel title="Assistant" detail={detail} className="assistant-panel">
      <div className="assistant-body">
        {readiness.phase === "loading" ? (
          <div className="assistant-notice" role="status" aria-live="polite">
            Checking provider and context pack…
          </div>
        ) : readiness.phase === "unconfigured" ? (
          <div className="assistant-notice assistant-notice-warning" role="status">
            <span className="response-rule" aria-hidden="true" />
            <div>
              <p className="state-title">Manual assistance needs setup</p>
              <p className="state-copy">{readiness.message}</p>
              <p className="state-copy">
                Configure OpenRouter in the desktop process and install a context pack
                in the application data directory.
              </p>
            </div>
          </div>
        ) : readiness.phase === "unavailable" ? (
          <div className="assistant-notice assistant-notice-warning" role="alert">
            <span className="response-rule" aria-hidden="true" />
            <div>
              <p className="state-title">Provider status is unavailable</p>
              <p className="state-copy">
                Reopen the app or check its configuration, then try again.
              </p>
            </div>
          </div>
        ) : (
          <>
            <div className="assistant-session-controls">
              <button
                className="assistant-new-session"
                type="button"
                onClick={() => {
                  void resetSession();
                }}
                disabled={busy || resetPending}
              >
                {resetPending ? "Resetting…" : "New session"}
              </button>
              {resetError !== null ? (
                <p className="assistant-error" role="alert">
                  {resetError}
                </p>
              ) : null}
            </div>
            <div
              ref={conversationRef}
              className="assistant-conversation"
              role="log"
              aria-label="Conversation"
              aria-live="polite"
              aria-relevant="additions text"
              onScroll={(event) => {
                const conversation = event.currentTarget;
                stickToBottom.current =
                  conversation.scrollHeight -
                    conversation.scrollTop -
                    conversation.clientHeight <
                  80;
              }}
            >
              {turns.length === 0 ? (
                <div className="assistant-empty">
                  <span className="response-rule" aria-hidden="true" />
                  <div>
                    <p className="state-title">Ready when you are</p>
                    <p className="state-copy">
                      Ask about the text you are working on. Relevant context is added
                      automatically.
                    </p>
                  </div>
                </div>
              ) : (
                turns.map((turn) => (
                  <article className="chat-turn" key={turn.id}>
                    <div className="chat-message chat-user-message">
                      <p className="chat-message-label">You</p>
                      <div className="chat-user-content">{turn.prompt}</div>
                    </div>
                    <div className="chat-message chat-assistant-message">
                      <p className="chat-message-label">Assistant</p>
                      {turn.answer.length > 0 ? (
                        <div className="assistant-markdown">
                          <ReactMarkdown remarkPlugins={[remarkGfm]}>
                            {turn.answer}
                          </ReactMarkdown>
                        </div>
                      ) : turn.phase === "starting" || turn.phase === "streaming" ? (
                        <p className="state-copy" role="status">
                          {turn.phase === "starting"
                            ? "Preparing a response…"
                            : "Generating response…"}
                        </p>
                      ) : turn.error !== null ? (
                        <p className="assistant-error" role="alert">
                          {turn.error}
                        </p>
                      ) : turn.phase === "cancelled" ? (
                        <p className="state-copy" role="status">
                          Request cancelled
                        </p>
                      ) : (
                        <p className="state-copy" role="status">
                          The provider returned an empty response. Try again.
                        </p>
                      )}
                      {turn.answer.length > 0 && turn.error !== null ? (
                        <p className="assistant-error" role="alert">
                          {turn.error}
                        </p>
                      ) : null}
                      {turn.answer.length > 0 && turn.phase === "cancelled" ? (
                        <p className="state-copy" role="status">
                          Request cancelled
                        </p>
                      ) : null}
                      {turn.answer.length > 0 && turn.phase === "completed" ? (
                        <p className="assistant-complete" role="status">
                          Response complete
                        </p>
                      ) : null}
                    </div>
                  </article>
                ))
              )}
            </div>

            <form className="assistant-composer" onSubmit={submit}>
              <label className="sr-only" htmlFor="manual-assistance-input">
                Ask for assistance
              </label>
              <textarea
                ref={composerRef}
                id="manual-assistance-input"
                name="text"
                rows={2}
                value={text}
                maxLength={16 * 1024}
                placeholder="Write or paste text to work with…"
                onChange={(event) => {
                  const composer = event.currentTarget;
                  setText(composer.value);
                  composer.style.height = "auto";
                  composer.style.height = `${String(Math.min(composer.scrollHeight, 160))}px`;
                }}
                onKeyDown={handleComposerKeyDown}
                disabled={busy || resetPending}
              />
              <div className="composer-footer">
                <span className="composer-hint">
                  Enter to send · Shift+Enter for a new line
                </span>
                <div className="composer-actions">
                  {phase === "streaming" ? (
                    <button
                      className="assistant-cancel"
                      type="button"
                      onClick={() => {
                        void cancel();
                      }}
                      disabled={cancelPending}
                    >
                      {cancelPending ? "Cancelling" : "Cancel"}
                    </button>
                  ) : null}
                  <button
                    className="assistant-send"
                    type="submit"
                    disabled={!text.trim() || busy || resetPending}
                  >
                    {phase === "starting" ? "Starting" : "Send"}
                  </button>
                </div>
              </div>
            </form>
          </>
        )}
      </div>
    </WorkspacePanel>
  );
}
