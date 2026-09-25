import { useEffect } from "react";

import { AssistantPanel } from "../features/assistant/AssistantPanel";
import { SessionPanel } from "../features/session/SessionPanel";
import { TranscriptPanel } from "../features/transcript/TranscriptPanel";
import { useApplicationStatusStore } from "../stores/application-status-store";

export function App() {
  const backend = useApplicationStatusStore((state) => state.backend);
  const loadStatus = useApplicationStatusStore((state) => state.loadStatus);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

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
            disabled
            title="Settings are planned for a later phase"
          >
            Settings
          </button>
        </div>
      </header>

      <div className="workspace">
        <SessionPanel />
        <div className="workspace-main">
          <TranscriptPanel />
          <AssistantPanel />
        </div>
      </div>
    </main>
  );
}
