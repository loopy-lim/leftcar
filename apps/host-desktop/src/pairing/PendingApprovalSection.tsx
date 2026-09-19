import { Check, Smartphone } from "lucide-react";
import { buttonVariants } from "../lib/variants";
import type { TranslationSchema } from "@leftcar/ui-tokens";

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
    <div className="paired-devices-section" role="alert">
      <div className="section-title-row">
        <div className="section-title-left">
          <Smartphone size={15} />
          <h4>{t.host.pairApprovalCardTitle}</h4>
          <span className="count-pill">{requests.length}</span>
        </div>
      </div>
      <p style={{ margin: "4px 0 8px", fontSize: 12, opacity: 0.75 }}>
        {t.host.pairApprovalCardHint}
      </p>
      <div className="device-rows-container">
        {requests.map((request) => (
          <div key={request.offer_id} className="device-row-item">
            <div className="device-row-left">
              <div className="device-icon-box">
                <Smartphone size={18} strokeWidth={2} />
              </div>
              <div className="device-row-main">
                <div className="device-row-header">
                  <span className="device-row-name">{request.device_name}</span>
                </div>
                <div className="device-row-meta">
                  <span className="device-row-date">{request.requested_at}</span>
                </div>
              </div>
            </div>
            <div className="device-row-actions">
              <button
                type="button"
                onClick={() => onApprove(request.offer_id)}
                className={buttonVariants({ variant: "primary", size: "sm" })}
                disabled={busyId !== null}
              >
                <Check size={13} /> {t.host.pairApprovalAllow}
              </button>
              <button
                type="button"
                onClick={() => onDeny(request.offer_id)}
                className={buttonVariants({ variant: "outlineDanger", size: "sm" })}
                disabled={busyId !== null}
              >
                {t.host.pairApprovalDeny}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
