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
          <span className="live-badge-pulse">
            <span className="status-dot" /> {t.host.liveBadge}
          </span>
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
        {sessions.map((session) => (
          <SessionCard
            key={session.session}
            session={session}
            inputPermission={inputPermission}
            inputBusy={inputBusy === session.session}
            showInspector={showInspector}
            t={t}
            onToggleInput={onToggleInput}
            onSetQuality={onSetQuality}
            qualityBusy={qualityBusy === session.session}
            onForceStop={onForceStop}
          />
        ))}
      </div>
    </div>
  );
}

export default StreamsListView;
