import { Check, Smartphone } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Notice, Text } from "../ui/primitives";
export interface PendingPairingView {
  offer_id: string;
  device_name: string;
  requested_at: string;
}
interface PendingApprovalSectionProps {
  requests: PendingPairingView[];
  busyId: string | null;
  onApprove: (offerId: string) => void;
  onDeny: (offerId: string) => void;
  t: TranslationSchema;
}
export default function PendingApprovalSection({
  requests,
  busyId,
  onApprove,
  onDeny,
  t,
}: PendingApprovalSectionProps) {
  if (requests.length === 0) return null;
  return (
    <Notice className="space-y-3" aria-label={t.host.pairApprovalCardTitle}>
      <div>
        <h3 className="text-body text-ink flex items-center gap-2 font-semibold">
          <Smartphone size={18} />
          {t.host.pairApprovalCardTitle}
          <Text variant="code" tone="muted">
            {requests.length}
          </Text>
        </h3>
        <p className="text-caption text-muted mt-1">
          {t.host.pairApprovalCardHint}
        </p>
      </div>
      <ul className="divide-y divide-outline">
        {requests.map((request) => (
          <li
            key={request.offer_id}
            className="flex flex-wrap items-center justify-between gap-3 py-3 first:pt-0 last:pb-0"
          >
            <div className="min-w-0 flex-1">
              <Text className="block break-words font-semibold">
                {request.device_name}
              </Text>
              <Text variant="caption" tone="muted" className="block">
                {request.requested_at}
              </Text>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                size="compact"
                busy={busyId === request.offer_id}
                disabled={busyId !== null}
                onClick={() => onApprove(request.offer_id)}
                aria-label={`${request.device_name}: ${t.host.pairApprovalAllow}`}
              >
                <Check size={16} />
                {busyId === request.offer_id
                  ? t.host.remoteInputProcessing
                  : t.host.pairApprovalAllow}
              </Button>
              <Button
                variant="secondary"
                size="compact"
                disabled={busyId !== null}
                onClick={() => onDeny(request.offer_id)}
                aria-label={`${request.device_name}: ${t.host.pairApprovalDeny}`}
              >
                {t.host.pairApprovalDeny}
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </Notice>
  );
}
