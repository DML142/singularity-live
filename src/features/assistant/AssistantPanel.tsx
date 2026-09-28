import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type SubmitEvent,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { WorkspacePanel } from "../../components/ui/WorkspacePanel";
import type { CropRect } from "../../lib/tauri/screen-assistance-client";
import {
  subscribeQuickSend,
  subscribeScreenshotSend,
} from "../../lib/tauri/shortcut-client";
import { useManualAssistanceStore } from "../../stores/manual-assistance-store";
import { useScreenAssistanceStore } from "../../stores/screen-assistance-store";
import { useVoiceInputStore } from "../../stores/voice-input-store";

interface AssistantPanelProps {
  readonly initialTask?: "text" | "screenshot" | undefined;
}

type ContextAction = "explain" | "tell me more" | "fix";

const CONTEXT_ACTIONS: readonly ContextAction[] = ["explain", "tell me more", "fix"];

export function AssistantPanel({ initialTask }: AssistantPanelProps) {
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
  const screenPhase = useScreenAssistanceStore((state) => state.phase);
  const screenCapabilities = useScreenAssistanceStore((state) => state.capabilities);
  const screenTargetKind = useScreenAssistanceStore((state) => state.targetKind);
  const screenPreview = useScreenAssistanceStore((state) => state.preview);
  const screenError = useScreenAssistanceStore((state) => state.error);
  const loadScreenCapabilities = useScreenAssistanceStore(
    (state) => state.loadCapabilities,
  );
  const captureScreen = useScreenAssistanceStore((state) => state.capture);
  const cancelScreenCapture = useScreenAssistanceStore((state) => state.cancelCapture);
  const cropScreen = useScreenAssistanceStore((state) => state.crop);
  const discardScreen = useScreenAssistanceStore((state) => state.discard);
  const sendScreen = useScreenAssistanceStore((state) => state.send);
  const clearScreenOnUnmount = useScreenAssistanceStore(
    (state) => state.clearOnUnmount,
  );
  const clearScreenForReset = useScreenAssistanceStore(
    (state) => state.clearForSessionReset,
  );
  const voicePhase = useVoiceInputStore((state) => state.phase);
  const voiceTranscript = useVoiceInputStore((state) => state.transcript);
  const voiceError = useVoiceInputStore((state) => state.error);
  const finalizedTranscript = useVoiceInputStore((state) => state.finalizedTranscript);
  const startVoiceInput = useVoiceInputStore((state) => state.start);
  const stopVoiceInput = useVoiceInputStore((state) => state.stop);
  const clearFinalizedTranscript = useVoiceInputStore(
    (state) => state.clearFinalizedTranscript,
  );
  const [text, setText] = useState("");
  const [contextAction, setContextAction] = useState<ContextAction | null>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const screenshotButtonRef = useRef<HTMLButtonElement>(null);
  const quickSendHandler = useRef<() => void>(() => {});
  const screenshotSendHandler = useRef<() => void>(() => {});
  const conversationRef = useRef<HTMLDivElement>(null);
  const screenshotImageRef = useRef<HTMLImageElement>(null);
  const cropStart = useRef<{ readonly x: number; readonly y: number } | null>(null);
  const [cropRect, setCropRect] = useState<CropRect | null>(null);
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
    void loadScreenCapabilities();
    return () => {
      void clearScreenOnUnmount();
    };
  }, [clearScreenOnUnmount, loadScreenCapabilities]);

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
    if (readiness.phase !== "ready") {
      return;
    }
    if (initialTask === "text") {
      composerRef.current?.focus();
    } else if (initialTask === "screenshot") {
      screenshotButtonRef.current?.focus();
    }
  }, [initialTask, readiness.phase]);

  useEffect(() => {
    const conversation = conversationRef.current;
    if (conversation !== null && stickToBottom.current) {
      conversation.scrollTop = conversation.scrollHeight;
    }
  }, [screenPhase, screenPreview, turns]);

  useEffect(() => {
    if (finalizedTranscript !== null) {
      composerRef.current?.focus();
    }
  }, [finalizedTranscript]);

  const draftText =
    finalizedTranscript === null
      ? text
      : text.trim().length === 0
        ? finalizedTranscript.text
        : `${text.trim()}\n${finalizedTranscript.text}`;

  const submitPrompt = (prompt: string) => {
    if (
      prompt.trim().length > 0 &&
      readiness.phase === "ready" &&
      phase !== "starting" &&
      phase !== "streaming" &&
      !resetPending
    ) {
      stickToBottom.current = true;
      setText("");
      setContextAction(null);
      if (finalizedTranscript !== null) {
        clearFinalizedTranscript(finalizedTranscript.id);
      }
      if (composerRef.current !== null) {
        composerRef.current.style.height = "56px";
      }
      void start(prompt);
    }
  };

  const busy = phase === "starting" || phase === "streaming";
  const screenBusy =
    screenPhase === "loadingTargets" ||
    screenPhase === "capturing" ||
    screenPhase === "cropping" ||
    screenPhase === "sending";
  const composedDraft =
    contextAction === null ? draftText : `${contextAction}: ${draftText}`.trimEnd();
  const sendCurrentDraft = () => {
    if (draftText.trim().length === 0 && screenPreview === null) {
      return;
    }
    if (screenPreview !== null) {
      if (readiness.phase === "ready" && !busy && !resetPending && !screenBusy) {
        stickToBottom.current = true;
        setText("");
        setContextAction(null);
        if (finalizedTranscript !== null) {
          clearFinalizedTranscript(finalizedTranscript.id);
        }
        if (composerRef.current !== null) {
          composerRef.current.style.height = "56px";
        }
        void sendScreen(composedDraft);
      }
      return;
    }
    submitPrompt(composedDraft);
  };

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    sendCurrentDraft();
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      sendCurrentDraft();
    }
  };

  useEffect(() => {
    quickSendHandler.current = () => {
      sendCurrentDraft();
    };
    screenshotSendHandler.current = () => {
      const currentScreen = useScreenAssistanceStore.getState();
      const currentAssistant = useManualAssistanceStore.getState();
      if (
        currentScreen.preview !== null &&
        currentAssistant.phase !== "starting" &&
        currentAssistant.phase !== "streaming" &&
        !currentAssistant.resetPending &&
        currentScreen.phase !== "capturing" &&
        currentScreen.phase !== "cropping" &&
        currentScreen.phase !== "sending"
      ) {
        sendCurrentDraft();
      }
    };
  });

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void subscribeQuickSend(() => {
      quickSendHandler.current();
    })
      .then((stopListening) => {
        if (mounted) {
          unlisten = stopListening;
        } else {
          stopListening();
        }
      })
      .catch(() => {});
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void subscribeScreenshotSend(() => {
      screenshotSendHandler.current();
    })
      .then((stopListening) => {
        if (mounted) {
          unlisten = stopListening;
        } else {
          stopListening();
        }
      })
      .catch(() => {});
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  const screenshotPoint = (event: PointerEvent<HTMLImageElement>) => {
    const image = event.currentTarget;
    const bounds = image.getBoundingClientRect();
    if (bounds.width === 0 || bounds.height === 0 || screenPreview === null) {
      return null;
    }
    return {
      x: Math.max(
        0,
        Math.min(
          screenPreview.width,
          Math.round(
            ((event.clientX - bounds.left) / bounds.width) * screenPreview.width,
          ),
        ),
      ),
      y: Math.max(
        0,
        Math.min(
          screenPreview.height,
          Math.round(
            ((event.clientY - bounds.top) / bounds.height) * screenPreview.height,
          ),
        ),
      ),
    };
  };
  const beginCropSelection = (event: PointerEvent<HTMLImageElement>) => {
    const point = screenshotPoint(event);
    if (point !== null && screenPreview !== null) {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      cropStart.current = point;
      setCropRect({ x: point.x, y: point.y, width: 1, height: 1 });
    }
  };
  const updateCropSelection = (event: PointerEvent<HTMLImageElement>) => {
    const startPoint = cropStart.current;
    const endPoint = screenshotPoint(event);
    if (startPoint === null || endPoint === null) {
      return;
    }
    setCropRect({
      x: Math.min(startPoint.x, endPoint.x),
      y: Math.min(startPoint.y, endPoint.y),
      width: Math.abs(startPoint.x - endPoint.x),
      height: Math.abs(startPoint.y - endPoint.y),
    });
  };
  const endCropSelection = (event: PointerEvent<HTMLImageElement>) => {
    updateCropSelection(event);
    cropStart.current = null;
  };
  const resetSessionAndScreenshot = async () => {
    setCropRect(null);
    if (await resetSession()) {
      setText("");
      setContextAction(null);
      clearScreenForReset();
    }
  };
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
                Configure a text provider in the desktop process and install a context
                pack in the application data directory.
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
                  void resetSessionAndScreenshot();
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
              {turns.length === 0 && screenPreview === null ? (
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
                <>
                  {turns.map((turn) => (
                    <article className="chat-turn" key={turn.id}>
                      <div className="chat-message chat-user-message">
                        <p className="chat-message-label">You</p>
                        <div className="chat-user-content">
                          {turn.prompt}
                          {turn.screenshotDataUrl !== null ? (
                            <img
                              className="chat-screenshot-image"
                              src={turn.screenshotDataUrl}
                              alt="Screenshot sent with this message"
                            />
                          ) : null}
                        </div>
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
                  ))}
                  {screenPreview !== null ? (
                    <article className="chat-turn screenshot-draft">
                      <div className="chat-message chat-user-message">
                        <p className="chat-message-label">Screenshot</p>
                        <div className="screen-preview-header">
                          <span>Temporary · removed after five minutes</span>
                          <span>
                            {screenPreview.width} × {screenPreview.height}
                          </span>
                        </div>
                        <div className="screen-preview-image-wrap">
                          <img
                            ref={screenshotImageRef}
                            className="screen-preview-image"
                            src={screenPreview.dataUrl}
                            alt="Temporary screenshot. Drag to select an area to crop."
                            draggable={false}
                            onPointerDown={beginCropSelection}
                            onPointerMove={updateCropSelection}
                            onPointerUp={endCropSelection}
                            onPointerCancel={endCropSelection}
                          />
                          {cropRect !== null &&
                          cropRect.width > 0 &&
                          cropRect.height > 0 ? (
                            <div
                              className="screen-crop-selection"
                              aria-label="Selected screenshot area"
                              style={{
                                left: `${String((cropRect.x / screenPreview.width) * 100)}%`,
                                top: `${String((cropRect.y / screenPreview.height) * 100)}%`,
                                width: `${String((cropRect.width / screenPreview.width) * 100)}%`,
                                height: `${String((cropRect.height / screenPreview.height) * 100)}%`,
                              }}
                            />
                          ) : null}
                        </div>
                        <p className="screen-assistance-copy">
                          Drag over the screenshot to crop it, then add a note below.
                        </p>
                        <div className="screen-preview-actions">
                          <button
                            className="screen-secondary-action"
                            type="button"
                            onClick={() => {
                              if (cropRect !== null) {
                                setCropRect(null);
                                void cropScreen(cropRect);
                              }
                            }}
                            disabled={
                              cropRect === null ||
                              cropRect.width < 1 ||
                              cropRect.height < 1 ||
                              screenBusy
                            }
                          >
                            Crop selected area
                          </button>
                          <button
                            className="screen-secondary-action"
                            type="button"
                            onClick={() => {
                              setCropRect(null);
                              void discardScreen();
                            }}
                            disabled={screenBusy}
                          >
                            Discard
                          </button>
                        </div>
                      </div>
                    </article>
                  ) : null}
                </>
              )}
              {screenPhase === "capturing" ? (
                <p className="screenshot-inline-status" role="status">
                  Capturing screenshot…
                </p>
              ) : null}
              {screenError !== null ? (
                <p className="screen-assistance-error" role="alert">
                  {screenError}
                </p>
              ) : null}
              {screenCapabilities?.permission === "denied" ? (
                <p className="screen-assistance-error" role="alert">
                  Screen capture permission was denied. Check system privacy settings.
                </p>
              ) : null}
              {screenCapabilities !== null &&
              screenCapabilities.targets.length === 0 ? (
                <p className="screen-assistance-error" role="status">
                  {screenCapabilities.message ??
                    "Screen capture is unavailable on this desktop."}
                </p>
              ) : null}
            </div>

            <form className="assistant-composer" onSubmit={submit}>
              <div
                className="assistant-context-actions"
                role="group"
                aria-label="Message context"
              >
                {CONTEXT_ACTIONS.map((action) => (
                  <button
                    className="assistant-context-action"
                    type="button"
                    key={action}
                    aria-pressed={contextAction === action}
                    disabled={busy || resetPending}
                    onClick={() => {
                      setContextAction((current) =>
                        current === action ? null : action,
                      );
                    }}
                  >
                    {action === "explain"
                      ? "Explain"
                      : action === "fix"
                        ? "Fix"
                        : "Tell me more"}
                  </button>
                ))}
              </div>
              <label className="sr-only" htmlFor="manual-assistance-input">
                Ask for assistance
              </label>
              <div className="assistant-composer-input">
                {contextAction !== null ? (
                  <span className="assistant-context-prefix" aria-hidden="true">
                    {contextAction}:
                  </span>
                ) : null}
                <textarea
                  ref={composerRef}
                  id="manual-assistance-input"
                  name="text"
                  rows={2}
                  value={draftText}
                  maxLength={
                    16 * 1024 -
                    (contextAction?.length ?? 0) -
                    (contextAction === null ? 0 : 2)
                  }
                  placeholder="Write or paste text to work with…"
                  onChange={(event) => {
                    const composer = event.currentTarget;
                    setText(composer.value);
                    if (finalizedTranscript !== null) {
                      clearFinalizedTranscript(finalizedTranscript.id);
                    }
                    composer.style.height = "auto";
                    composer.style.height = `${String(Math.min(composer.scrollHeight, 160))}px`;
                  }}
                  onKeyDown={handleComposerKeyDown}
                  disabled={busy || resetPending}
                />
              </div>
              {voicePhase === "recording" ? (
                <p className="voice-capture-status" role="status">
                  {voiceTranscript.length > 0 ? voiceTranscript : "Listening…"}
                </p>
              ) : null}
              {voiceError !== null ? (
                <p className="voice-capture-error" role="alert">
                  {voiceError}
                </p>
              ) : null}
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
                  {screenPhase === "capturing" ? (
                    <button
                      className="screen-secondary-action"
                      type="button"
                      onClick={() => void cancelScreenCapture()}
                    >
                      Cancel capture
                    </button>
                  ) : (
                    <button
                      ref={screenshotButtonRef}
                      className="screen-secondary-action"
                      type="button"
                      onClick={() => {
                        setCropRect(null);
                        void captureScreen();
                      }}
                      disabled={
                        screenCapabilities === null ||
                        screenCapabilities.permission === "denied" ||
                        !screenCapabilities.targets.includes(screenTargetKind) ||
                        screenPreview !== null ||
                        screenBusy ||
                        busy ||
                        resetPending
                      }
                    >
                      {screenTargetKind === "monitor"
                        ? "Screenshot · Screen"
                        : "Screenshot · Window"}
                    </button>
                  )}
                  <button
                    className="assistant-record"
                    type="button"
                    aria-pressed={voicePhase === "recording"}
                    disabled={voicePhase === "connecting" || voicePhase === "stopping"}
                    onClick={() => {
                      if (voicePhase === "recording") {
                        void stopVoiceInput();
                      } else {
                        void startVoiceInput();
                      }
                    }}
                  >
                    {voicePhase === "connecting"
                      ? "Connecting…"
                      : voicePhase === "recording"
                        ? "Stop"
                        : voicePhase === "stopping"
                          ? "Finalizing…"
                          : "Record"}
                  </button>
                  <button
                    className="assistant-send"
                    type="submit"
                    disabled={
                      (!draftText.trim() && screenPreview === null) ||
                      busy ||
                      resetPending ||
                      screenBusy
                    }
                  >
                    {phase === "starting"
                      ? "Starting"
                      : screenPreview === null
                        ? "Send"
                        : "Send screenshot"}
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
