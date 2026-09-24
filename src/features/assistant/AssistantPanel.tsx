import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type SubmitEvent,
} from "react";

import { WorkspacePanel } from "../../components/ui/WorkspacePanel";
import { useManualAssistanceStore } from "../../stores/manual-assistance-store";

export function AssistantPanel() {
  const readiness = useManualAssistanceStore((state) => state.readiness);
  const phase = useManualAssistanceStore((state) => state.phase);
  const answer = useManualAssistanceStore((state) => state.answer);
  const error = useManualAssistanceStore((state) => state.error);
  const cancelPending = useManualAssistanceStore((state) => state.cancelPending);
  const initialize = useManualAssistanceStore((state) => state.initialize);
  const start = useManualAssistanceStore((state) => state.start);
  const cancel = useManualAssistanceStore((state) => state.cancel);
  const [text, setText] = useState("");
  const composerRef = useRef<HTMLTextAreaElement>(null);
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

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (text.trim().length > 0) {
      void start(text);
    }
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      if (text.trim().length > 0) {
        void start(text);
      }
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
            <div className="assistant-response">
              <div className="response-rule" aria-hidden="true" />
              <div className="assistant-response-content">
                {answer.length > 0 ? (
                  <div
                    className="assistant-answer"
                    role="log"
                    aria-label="Assistant response"
                    aria-live="polite"
                    aria-relevant="additions"
                  >
                    {answer}
                  </div>
                ) : busy ? (
                  <p className="state-copy" role="status" aria-live="polite">
                    Preparing a response…
                  </p>
                ) : phase === "cancelled" ? (
                  <p className="state-copy" role="status">
                    Request cancelled
                  </p>
                ) : phase === "failed" && error !== null ? (
                  <p className="assistant-error" role="alert">
                    {error}
                  </p>
                ) : phase === "completed" ? (
                  <p className="state-copy" role="status">
                    The provider returned an empty response. Try again.
                  </p>
                ) : (
                  <>
                    <p className="state-title">Ready when you are</p>
                    <p className="state-copy">
                      Ask about the text you are working on. Relevant context is added
                      automatically.
                    </p>
                  </>
                )}
                {phase === "completed" ? (
                  <p className="assistant-complete" role="status">
                    Response complete
                  </p>
                ) : null}
                {phase === "failed" && answer.length > 0 && error !== null ? (
                  <p className="assistant-error" role="alert">
                    {error}
                  </p>
                ) : null}
                {busy && error !== null ? (
                  <p className="assistant-error" role="alert">
                    {error}
                  </p>
                ) : null}
              </div>
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
                  setText(event.currentTarget.value);
                }}
                onKeyDown={handleComposerKeyDown}
                disabled={busy}
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
                    disabled={!text.trim() || busy}
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
