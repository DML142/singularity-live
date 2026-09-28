import { useCallback, useEffect, useRef, useState } from "react";

import { AssistantPanel } from "../features/assistant/AssistantPanel";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import {
  getScreenshotPreferences,
  getWindowOpacity,
} from "../lib/tauri/customization-client";
import { subscribeMinModeToggle } from "../lib/tauri/shortcut-client";
import { useApplicationStatusStore } from "../stores/application-status-store";
import { useScreenAssistanceStore } from "../stores/screen-assistance-store";
import { listenForHotkeyCapture } from "../lib/tauri/hotkey-capture-client";
import { useManualAssistanceStore } from "../stores/manual-assistance-store";
import { useVoiceInputStore } from "../stores/voice-input-store";

export function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [minMode, setMinMode] = useState(false);
  const devAssistTask = import.meta.env.VITE_SINGULARITY_LIVE_DEV_ASSIST_TASK;
  const backend = useApplicationStatusStore((state) => state.backend);
  const loadStatus = useApplicationStatusStore((state) => state.loadStatus);
  const loadReadiness = useManualAssistanceStore((state) => state.loadReadiness);
  const initializeVoiceInput = useVoiceInputStore((state) => state.initialize);
  const voiceSource = useVoiceInputStore((state) => state.source);
  const voiceDevices = useVoiceInputStore((state) => state.devices);
  const microphoneDeviceId = useVoiceInputStore((state) => state.microphoneDeviceId);
  const acceptHotkeyCapture = useScreenAssistanceStore(
    (state) => state.acceptHotkeyCapture,
  );
  const setScreenshotTargetKind = useScreenAssistanceStore(
    (state) => state.setTargetKind,
  );
  const handleHotkeyCapture = useCallback(
    (event: Parameters<typeof acceptHotkeyCapture>[0]) => {
      acceptHotkeyCapture(event);
      setSettingsOpen(false);
    },
    [acceptHotkeyCapture],
  );
  const audioInputLabel =
    voiceSource === "system_audio"
      ? "System audio"
      : (voiceDevices.find((device) => device.id === microphoneDeviceId)?.label ??
        voiceDevices.find((device) => device.isDefault)?.label ??
        "Default microphone");
  const wasSettingsOpen = useRef(settingsOpen);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  useEffect(() => {
    if (wasSettingsOpen.current && !settingsOpen) {
      void loadReadiness();
    }
    wasSettingsOpen.current = settingsOpen;
  }, [loadReadiness, settingsOpen]);

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
    let active = true;
    void getScreenshotPreferences()
      .then((preferences) => {
        if (active) {
          setScreenshotTargetKind(preferences.targetKind);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [setScreenshotTargetKind]);

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

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void subscribeMinModeToggle(() => {
      setMinMode((current) => !current);
      setSettingsOpen(false);
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

  return (
    <main
      className="app-shell bg-[var(--color-canvas)] text-[var(--color-text)]"
      data-min-mode={minMode ? "true" : "false"}
    >
      {!minMode ? (
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
            <span className="audio-input-status" title={audioInputLabel}>
              {audioInputLabel}
            </span>
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
      ) : null}

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
