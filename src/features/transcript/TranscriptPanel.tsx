import { WorkspacePanel } from "../../components/ui/WorkspacePanel";

export function TranscriptPanel() {
  return (
    <WorkspacePanel
      title="Transcript"
      detail="Live context"
      className="transcript-panel"
    >
      <div className="empty-state">
        <span className="empty-index" aria-hidden="true">
          00:00
        </span>
        <div>
          <p className="state-title">No transcript yet</p>
          <p className="state-copy">
            Spoken and typed context will appear here after a session starts.
          </p>
        </div>
      </div>
    </WorkspacePanel>
  );
}
