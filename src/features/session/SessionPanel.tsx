import { WorkspacePanel } from "../../components/ui/WorkspacePanel";

export function SessionPanel() {
  return (
    <aside className="session-rail" aria-label="Session overview">
      <WorkspacePanel title="Session" detail="Idle">
        <div className="session-state">
          <div className="state-orbit" aria-hidden="true">
            <span />
          </div>
          <div>
            <p className="state-title">No active session</p>
            <p className="state-copy">
              Capture and assistance controls arrive in later roadmap phases.
            </p>
          </div>
        </div>
        <dl className="session-facts">
          <div>
            <dt>Capture</dt>
            <dd>Off</dd>
          </div>
          <div>
            <dt>Context</dt>
            <dd>None loaded</dd>
          </div>
        </dl>
      </WorkspacePanel>
    </aside>
  );
}
