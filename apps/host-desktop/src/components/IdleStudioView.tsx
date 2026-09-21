import { Info, Monitor, QrCode } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "../lib/variants";

export interface IdleStudioViewProps {
  t: TranslationSchema;
  onOpenPairing: () => void;
}

export function IdleStudioView({ t, onOpenPairing }: IdleStudioViewProps) {
  return (
    <div className="idle-center-container">
      <div className="idle-center-card">
        <div className="idle-center-icon-box">
          <Monitor size={28} strokeWidth={2} />
        </div>
        <div className="idle-center-text">
          <h2>{t.host.idleTitle}</h2>
          <p>{t.host.idleDesc}</p>
        </div>

        <button
          className={buttonVariants({ variant: "primary", size: "lg" })}
          onClick={onOpenPairing}
          title={`${t.host.btnCreatePairing} (${t.host.shortcutPair})`}
        >
          <QrCode size={16} />
          <span>{t.host.btnCreatePairing}</span>
          <span
            className="kbd-shortcut"
            style={{
              marginLeft: 6,
              background: "rgba(255,255,255,0.2)",
              color: "inherit",
              borderColor: "rgba(255,255,255,0.3)",
            }}
          >
            {t.host.shortcutPair}
          </span>
        </button>

        <span className="idle-center-hint">
          <Info size={13} />
          {t.host.idleHint}
        </span>
      </div>
    </div>
  );
}

export default IdleStudioView;
