import SessionEncoderDiagnostics from "./SessionEncoderDiagnostics";
import SessionPipelineDiagnostics from "./SessionPipelineDiagnostics";
import type { SessionRow } from "./sessionTypes";

interface SessionInspectorProps {
  session: SessionRow;
  transportLabel: string;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
}
export default function SessionInspector({
  session,
  transportLabel,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
}: SessionInspectorProps) {
  return (
    <div className="inspector-panel">
      <div className="inspector-group">
        <div className="inspector-group-header">
          <span className="inspector-header">파이프라인 지표</span>
        </div>
        <div className="inspector-grid">
          <SessionPipelineDiagnostics
            session={session}
            transportLabel={transportLabel}
          />
        </div>
      </div>

      <div className="inspector-divider" />

      <div className="inspector-group">
        <div className="inspector-group-header">
          <span className="inspector-header">인코더 & 화질 제어</span>
        </div>
        <div className="inspector-grid">
          <SessionEncoderDiagnostics
            session={session}
            qualitySupported={qualitySupported}
            qualityPercent={qualityPercent}
            qualityBusy={qualityBusy}
            onSetQuality={onSetQuality}
          />
        </div>
      </div>
    </div>
  );
}
