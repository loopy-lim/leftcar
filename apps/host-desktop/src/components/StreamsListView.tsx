import { ChevronDown, ChevronUp } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import { buttonVariants } from "../lib/variants";
import { SessionCard } from "./SessionCard";

export interface StreamsListViewProps {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  showInspector: boolean;
  /** 입력 허용을 요청 중인 세션 id(뷰어 잠금 배너 탭). */
  inputRequestSessions?: number[];
  t: TranslationSchema;
  onToggleInspector: () => void;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: number | null;
  onForceStop: (session: SessionRow) => void;
}

export function StreamsListView({
  sessions,
  inputPermission,
  inputBusy,
  showInspector,
  inputRequestSessions = [],
  t,
  onToggleInspector,
  onToggleInput,
  onSetQuality,
  qualityBusy,
  onForceStop,
}: StreamsListViewProps) {
  return (
    <div className="streams-section">
      <div className="streams-section-header">
        <div className="streams-header-left">
          <h2>{interpolate(t.host.activeSectionTitle, { count: sessions.length })}</h2>
        </div>
        <button
          className={buttonVariants({ variant: "link" })}
          onClick={onToggleInspector}
          style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
        >
          {showInspector ? (
            <>{t.host.hideMetrics} <ChevronUp size={14} /></>
          ) : (
            <>{t.host.showMetrics} <ChevronDown size={14} /></>
          )}
        </button>
      </div>

      <div className="stream-cards-container">
        {(() => {
          const requestedSessions = new Set(inputRequestSessions);
          return sessions.map((session) => (
          <SessionCard
            key={session.session}
            session={session}
            inputPermission={inputPermission}
            inputBusy={inputBusy === session.session}
            showInspector={showInspector}
            inputRequested={requestedSessions.has(session.session)}
            t={t}
            onToggleInput={onToggleInput}
            onSetQuality={onSetQuality}
            qualityBusy={qualityBusy === session.session}
            onForceStop={onForceStop}
          />
          ));
        })()}
      </div>
    </div>
  );
}

export default StreamsListView;
