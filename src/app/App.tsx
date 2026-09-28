import { useCallback, useEffect, useState } from "react";

import { AssistantPanel } from "../features/assistant/AssistantPanel";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import { getWindowOpacity } from "../lib/tauri/customization-client";
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
    let active = true;
    void getWindowOpacity()
      .then((opacity) => {
        if (active) {
          document.documentElement.dataset.appOpacity = String(opacity);
        }
      })
      .catch(() => {
        if (active) {
          document.documentElement.dataset.appOpacity = "100";
        }
      });
    return () => {
      active = false;
    };
  }, []);

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

      <div className="workspace" hidden={settingsOpen} aria-hidden={settingsOpen}>
        <div className="workspace-main">
          <AssistantPanel initialTask={devAssistTask} />
        </div>
      </div>
      {settingsOpen ? (
        <SettingsPanel
          onBack={() => {
            setSettingsOpen(false);
          }}
        />
      ) : null}
    </main>
  );
}
