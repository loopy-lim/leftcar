import { useId } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import type { SessionActionKind } from "../hooks/useSessionActions";
import { Button } from "../ui/primitives";
import { SessionCard } from "./SessionCard";
export interface StreamsListViewProps {
  sessions: SessionRow[];
  inputPermission: boolean;
  inputBusy: number | "permission" | null;
  showInspector: boolean;
  inputRequestSessions?: number[];
  t: TranslationSchema;
  onToggleInspector: () => void;
  onToggleInput: (session: SessionRow) => Promise<void>;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  qualityBusy: number | null;
  actionsBusy?: Record<number, SessionActionKind>;
  actionErrors?: Record<number, string>;
  onRetryAction?: (session: number) => void;
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
  actionsBusy = {},
  actionErrors = {},
  onRetryAction,
  onForceStop,
}: StreamsListViewProps) {
  const requestedSessions = new Set(inputRequestSessions);
  const panelId = useId();
  return (
    <section
      className="space-y-3"
      aria-label={interpolate(t.host.activeSectionTitle, {
        count: sessions.length,
      })}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-title text-ink font-semibold">
          {interpolate(t.host.activeSectionTitle, { count: sessions.length })}
        </h2>
        <Button
          variant="ghost"
          size="compact"
          onClick={onToggleInspector}
          aria-expanded={showInspector}
          aria-controls={panelId}
        >
          {showInspector ? t.host.hideMetrics : t.host.showMetrics}
          {showInspector ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
        </Button>
      </div>
      <div id={panelId}>
        {sessions.map((session) => (
          <SessionCard
            key={session.session}
            session={session}
            inputPermission={inputPermission}
            inputBusy={
              actionsBusy[session.session] === "input" ||
              inputBusy === session.session
            }
            showInspector={showInspector}
            inputRequested={requestedSessions.has(session.session)}
            t={t}
            onToggleInput={onToggleInput}
            onSetQuality={onSetQuality}
            qualityBusy={
              actionsBusy[session.session] === "quality" ||
              qualityBusy === session.session
            }
            stopping={actionsBusy[session.session] === "stop"}
            actionError={actionErrors[session.session]}
            onRetry={
              onRetryAction ? () => onRetryAction(session.session) : undefined
            }
            onForceStop={onForceStop}
          />
        ))}
      </div>
    </section>
  );
}
export default StreamsListView;
