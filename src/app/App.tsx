import { useCallback, useEffect, useState } from "react";

import { AssistantPanel } from "../features/assistant/AssistantPanel";
import { SessionPanel } from "../features/session/SessionPanel";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import { TranscriptPanel } from "../features/transcript/TranscriptPanel";
import { useApplicationStatusStore } from "../stores/application-status-store";
import { useScreenAssistanceStore } from "../stores/screen-assistance-store";
import { listenForHotkeyCapture } from "../lib/tauri/hotkey-capture-client";
import { useVoiceInputStore } from "../stores/voice-input-store";

export function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const devAssistTask = import.meta.env.VITE_SINGULARITY_LIVE_DEV_ASSIST_TASK;
  const backend = useApplicationStatusStore((state) => state.backend);
  const loadStatus = useApplicationStatusStore((state) => state.loadStatus);
  const initializeVoiceInput = useVoiceInputStore((state) => state.initialize);
  const acceptHotkeyCapture = useScreenAssistanceStore(
    (state) => state.acceptHotkeyCapture,
  );
  const handleHotkeyCapture = useCallback(
    (event: Parameters<typeof acceptHotkeyCapture>[0]) => {
      acceptHotkeyCapture(event);
      setSettingsOpen(false);
    },
    [acceptHotkeyCapture],
  );

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void initializeVoiceInput().then((stopListening) => {
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
  }, [initializeVoiceInput]);

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void listenForHotkeyCapture(handleHotkeyCapture)
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
  }, [handleHotkeyCapture]);

  return (
    <main className="app-shell bg-[var(--color-canvas)] text-[var(--color-text)]">
      <header className="app-header">
        <div className="brand-lockup">
          <span className="signal-mark" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          <div>
            <h1>Singularity Live</h1>
            <p>Context copilot</p>
          </div>
        </div>
        <div className="header-actions">
          <div
            className="backend-status"
            data-state={backend.phase}
            role="status"
            aria-live="polite"
          >
            <span aria-hidden="true" />
            {backend.label}
          </div>
          <button
            className="settings-button"
            type="button"
            aria-pressed={settingsOpen}
            onClick={() => {
              setSettingsOpen((open) => !open);
            }}
          >
            Settings
          </button>
        </div>
      </header>

      {settingsOpen ? (
        <SettingsPanel
          onBack={() => {
            setSettingsOpen(false);
          }}
        />
      ) : (
        <div className="workspace">
          <SessionPanel />
          <div className="workspace-main">
            <TranscriptPanel />
            <AssistantPanel initialTask={devAssistTask} />
          </div>
        </div>
      )}
    </main>
  );
}
