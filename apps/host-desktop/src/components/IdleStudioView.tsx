import type { TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Text } from "../ui/primitives";
export interface IdleStudioViewProps {
  t: TranslationSchema;
  onOpenPairing: () => void;
  screenReady?: boolean;
}
export function IdleStudioView({
  t,
  onOpenPairing,
  screenReady = true,
}: IdleStudioViewProps) {
  return (
    <div className="flex min-h-60 flex-1 flex-col items-center justify-center px-4 py-6 text-center">
      <div className="flex w-full max-w-md flex-col items-center gap-4">
        <div className="space-y-2">
          <h2 className="text-heading text-ink font-semibold">
            {t.host.idleTitle}
          </h2>
          <p className="text-body text-muted">{t.host.idleDesc}</p>
        </div>
        <Button
          variant={screenReady ? "primary" : "secondary"}
          onClick={onOpenPairing}
          title={`${t.host.btnCreatePairing} (${t.host.shortcutPair})}`}
        >
          {t.host.btnCreatePairing}
        </Button>
        <Text variant="caption" tone="muted">
          {t.host.setupPairStep}
        </Text>
      </div>
    </div>
  );
}
export default IdleStudioView;
