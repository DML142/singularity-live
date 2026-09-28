import { WorkspacePanel } from "../../components/ui/WorkspacePanel";
import { useVoiceInputStore } from "../../stores/voice-input-store";

export function TranscriptPanel() {
  const phase = useVoiceInputStore((state) => state.phase);
  const transcript = useVoiceInputStore((state) => state.transcript);
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
          {transcript.length > 0 ? (
            <p className="state-copy" aria-live="polite">
              {transcript}
            </p>
          ) : (
            <>
              <p className="state-title">
                {phase === "recording" ? "Listening…" : "No transcript yet"}
              </p>
              <p className="state-copy">
                Start voice input to see the live transcript here.
              </p>
            </>
          )}
        </div>
      </div>
    </WorkspacePanel>
  );
}
