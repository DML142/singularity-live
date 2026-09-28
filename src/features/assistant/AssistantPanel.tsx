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
import { useManualAssistanceStore } from "../../stores/manual-assistance-store";
import { useScreenAssistanceStore } from "../../stores/screen-assistance-store";
import { useVoiceInputStore } from "../../stores/voice-input-store";

interface AssistantPanelProps {
  readonly initialTask?: "text" | "screenshot" | undefined;
}

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
  const screenTargets = useScreenAssistanceStore((state) => state.targets);
  const selectedScreenTargetId = useScreenAssistanceStore(
    (state) => state.selectedTargetId,
  );
  const captureOperationId = useScreenAssistanceStore((state) => state.operationId);
  const screenPreview = useScreenAssistanceStore((state) => state.preview);
  const screenError = useScreenAssistanceStore((state) => state.error);
  const loadScreenCapabilities = useScreenAssistanceStore(
    (state) => state.loadCapabilities,
  );
  const loadScreenTargets = useScreenAssistanceStore((state) => state.loadTargets);
  const selectScreenTarget = useScreenAssistanceStore((state) => state.selectTarget);
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
  const voiceSource = useVoiceInputStore((state) => state.source);
  const voiceError = useVoiceInputStore((state) => state.error);
  const finalizedTranscript = useVoiceInputStore((state) => state.finalizedTranscript);
  const setVoiceSource = useVoiceInputStore((state) => state.setSource);
  const startVoiceInput = useVoiceInputStore((state) => state.start);
  const stopVoiceInput = useVoiceInputStore((state) => state.stop);
  const clearFinalizedTranscript = useVoiceInputStore(
    (state) => state.clearFinalizedTranscript,
  );
  const [text, setText] = useState("");
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const screenAssistanceRef = useRef<HTMLElement>(null);
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
      screenAssistanceRef.current?.focus();
    }
  }, [initialTask, readiness.phase]);

  useEffect(() => {
    const conversation = conversationRef.current;
    if (conversation !== null && stickToBottom.current) {
      conversation.scrollTop = conversation.scrollHeight;
    }
  }, [turns]);

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
    if (prompt.trim().length > 0) {
      stickToBottom.current = true;
      setText("");
      if (finalizedTranscript !== null) {
        clearFinalizedTranscript(finalizedTranscript.id);
      }
      if (composerRef.current !== null) {
        composerRef.current.style.height = "56px";
      }
      void start(prompt);
    }
  };

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    submitPrompt(draftText);
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submitPrompt(draftText);
    }
  };

  const busy = phase === "starting" || phase === "streaming";
  const hasPriorConversation = turns.some((turn) => turn.phase === "completed");
  const screenBusy =
    screenPhase === "loadingTargets" ||
    screenPhase === "capturing" ||
    screenPhase === "cropping" ||
    screenPhase === "sending";
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
        <section className="screen-assistance" aria-label="Voice input">
          <div className="screen-assistance-heading">
            <div>
              <p className="screen-assistance-title">Voice input · Soniox</p>
              <p className="screen-assistance-copy">
                Transcribed speech appears in the composer for review. Sending stays
                manual.
              </p>
            </div>
          </div>
          <div className="screen-assistance-controls">
            <label className="screen-source-label" htmlFor="voice-source-kind">
              Source
            </label>
            <select
              id="voice-source-kind"
              value={voiceSource}
              disabled={
                voicePhase === "connecting" ||
                voicePhase === "recording" ||
                voicePhase === "stopping"
              }
              onChange={(event) => {
                if (
                  event.currentTarget.value === "microphone" ||
                  event.currentTarget.value === "system_audio"
                ) {
                  void setVoiceSource(event.currentTarget.value);
                }
              }}
            >
              <option value="microphone">Microphone</option>
              <option value="system_audio">System audio</option>
            </select>
            <button
              className="screen-capture-action"
              type="button"
              onClick={() => {
                if (voicePhase === "recording") {
                  void stopVoiceInput();
                } else {
                  void startVoiceInput();
                }
              }}
              disabled={voicePhase === "connecting" || voicePhase === "stopping"}
            >
              {voicePhase === "connecting"
                ? "Connecting…"
                : voicePhase === "recording"
                  ? "Stop voice input"
                  : voicePhase === "stopping"
                    ? "Finalizing…"
                    : "Start voice input"}
            </button>
            {voicePhase === "recording" ? (
              <span className="screen-permission-hint" role="status">
                Voice input active
              </span>
            ) : null}
          </div>
          {voiceError !== null ? (
            <p className="screen-assistance-error" role="alert">
              {voiceError}
            </p>
          ) : null}
        </section>
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
            <section
              className="screen-assistance"
              aria-label="Screen assistance"
              ref={screenAssistanceRef}
              tabIndex={-1}
            >
              <div className="screen-assistance-heading">
                <div>
                  <p className="screen-assistance-title">Temporary screen help</p>
                  <p className="screen-assistance-copy">
                    Capture only when you choose, review it, then send it with the
                    previous text request.
                  </p>
                </div>
                {screenCapabilities?.permission === "user_prompt" ? (
                  <span className="screen-permission-hint">
                    System approval may be requested
                  </span>
                ) : null}
              </div>

              {screenPhase === "loading" ? (
                <p className="screen-assistance-status" role="status">
                  Checking screen capture support…
                </p>
              ) : screenPhase === "unavailable" || screenCapabilities === null ? (
                <p className="screen-assistance-error" role="alert">
                  {screenError ?? "Screen capture support is unavailable"}
                </p>
              ) : screenCapabilities.permission === "denied" ? (
                <p className="screen-assistance-error" role="alert">
                  Screen capture permission was denied. Check system privacy settings.
                </p>
              ) : screenCapabilities.targets.length === 0 ? (
                <p className="screen-assistance-error" role="alert">
                  {screenCapabilities.message ??
                    "Screen capture is unavailable on this desktop"}
                </p>
              ) : (
                <>
                  <div className="screen-assistance-controls">
                    <label className="screen-source-label" htmlFor="screen-source-kind">
                      Source
                    </label>
                    <select
                      id="screen-source-kind"
                      value={screenTargetKind}
                      disabled={
                        screenBusy || screenPreview !== null || busy || resetPending
                      }
                      onChange={(event) => {
                        setCropRect(null);
                        const kind = event.currentTarget.value;
                        if (kind === "monitor" || kind === "window") {
                          void loadScreenTargets(kind);
                        }
                      }}
                    >
                      {screenCapabilities.targets.includes("monitor") ? (
                        <option value="monitor">Screen</option>
                      ) : null}
                      {screenCapabilities.targets.includes("window") ? (
                        <option value="window">Window</option>
                      ) : null}
                    </select>
                    <button
                      className="screen-secondary-action"
                      type="button"
                      onClick={() => {
                        void loadScreenTargets(screenTargetKind);
                      }}
                      disabled={
                        screenBusy || screenPreview !== null || busy || resetPending
                      }
                    >
                      {screenPhase === "loadingTargets"
                        ? "Loading sources…"
                        : "Choose source"}
                    </button>
                    {screenPhase === "capturing" ? (
                      <button
                        className="screen-secondary-action"
                        type="button"
                        onClick={() => {
                          void cancelScreenCapture();
                        }}
                        disabled={captureOperationId === null}
                      >
                        Cancel capture
                      </button>
                    ) : null}
                  </div>

                  {screenTargets.length > 0 && screenPreview === null ? (
                    <div className="screen-target-row">
                      <label className="sr-only" htmlFor="screen-capture-target">
                        Available{" "}
                        {screenTargetKind === "monitor" ? "screens" : "windows"}
                      </label>
                      <select
                        id="screen-capture-target"
                        value={selectedScreenTargetId}
                        disabled={screenBusy || busy || resetPending}
                        onChange={(event) => {
                          selectScreenTarget(event.currentTarget.value);
                        }}
                      >
                        {screenTargets.map((target) => (
                          <option key={target.id} value={target.id}>
                            {target.label}
                          </option>
                        ))}
                      </select>
                      <button
                        className="screen-capture-action"
                        type="button"
                        onClick={() => {
                          void captureScreen();
                        }}
                        disabled={screenBusy || busy || resetPending}
                      >
                        {screenTargetKind === "monitor"
                          ? "Capture screen"
                          : "Capture window"}
                      </button>
                    </div>
                  ) : null}
                  {screenPhase === "selecting" && screenTargets.length === 0 ? (
                    <p className="screen-assistance-status" role="status">
                      No {screenTargetKind === "monitor" ? "screens" : "windows"} are
                      available for capture. Try another source type.
                    </p>
                  ) : null}
                </>
              )}

              {screenPhase === "capturing" ? (
                <p
                  className="screen-assistance-status"
                  role="status"
                  aria-live="polite"
                >
                  Waiting for the screen capture to finish…
                </p>
              ) : null}
              {screenError !== null ? (
                <p className="screen-assistance-error" role="alert">
                  {screenError}
                </p>
              ) : null}
              {screenPhase === "cancelled" ? (
                <p className="screen-assistance-status" role="status">
                  Screenshot capture cancelled
                </p>
              ) : null}
              {screenPhase === "sent" ? (
                <p className="screen-assistance-status" role="status">
                  Screenshot sent with the previous text request
                </p>
              ) : null}
              {screenPreview !== null ? (
                <div className="screen-preview-card">
                  <div className="screen-preview-header">
                    <span>Preview · deleted automatically after five minutes</span>
                    <span>
                      {screenPreview.width} × {screenPreview.height}
                    </span>
                  </div>
                  <div className="screen-preview-image-wrap">
                    <img
                      ref={screenshotImageRef}
                      className="screen-preview-image"
                      src={screenPreview.dataUrl}
                      alt="Temporary screenshot preview. Drag to select an area to crop."
                      draggable={false}
                      onPointerDown={beginCropSelection}
                      onPointerMove={updateCropSelection}
                      onPointerUp={endCropSelection}
                      onPointerCancel={endCropSelection}
                    />
                    {cropRect !== null && cropRect.width > 0 && cropRect.height > 0 ? (
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
                    Drag over a region to crop it, or send the full preview with your
                    previous question.
                  </p>
                  {!hasPriorConversation ? (
                    <p className="screen-assistance-status" role="status">
                      Ask a text question and wait for its response before sending a
                      screenshot.
                    </p>
                  ) : null}
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
                      className="screen-capture-action"
                      type="button"
                      onClick={() => {
                        setCropRect(null);
                        void sendScreen();
                      }}
                      disabled={
                        !hasPriorConversation || busy || resetPending || screenBusy
                      }
                    >
                      Send screenshot
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
              ) : null}
            </section>
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
                value={draftText}
                maxLength={16 * 1024}
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
                    disabled={!draftText.trim() || busy || resetPending}
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
