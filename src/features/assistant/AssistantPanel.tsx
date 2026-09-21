import { WorkspacePanel } from "../../components/ui/WorkspacePanel";

export function AssistantPanel() {
  return (
    <WorkspacePanel title="Assistant" detail="Standby" className="assistant-panel">
      <div className="assistant-empty">
        <div className="response-rule" aria-hidden="true" />
        <div>
          <p className="state-title">Ready when you are</p>
          <p className="state-copy">
            Suggestions will appear when a session is active.
          </p>
        </div>
      </div>
    </WorkspacePanel>
  );
}
