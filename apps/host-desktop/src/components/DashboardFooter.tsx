import { Check, Copy } from "lucide-react";
import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import { formatHostAddress } from "../hostState";
import { Button, Text } from "../ui/primitives";
export interface DashboardFooterProps {
  controlPort: number;
  lanIp: string | null;
  copiedToast: boolean;
  clipboardShare: boolean;
  privacyCurtain: boolean;
  t: TranslationSchema;
  onCopyAddress: () => void;
}
export function DashboardFooter(props: DashboardFooterProps) {
  const { t } = props;
  const addressText = formatHostAddress(props.lanIp, props.controlPort);
  const privacyLabels = [
    props.clipboardShare ? t.host.clipboardShareLabel : null,
    props.privacyCurtain ? t.host.privacyCurtainLabel : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <footer className="flex min-h-14 shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t border-outline bg-surface px-4 py-1">
      <Button
        variant="ghost"
        size="compact"
        disabled={!props.lanIp}
        onClick={props.onCopyAddress}
        title={t.host.computerAddressLabel}
        className="max-w-full flex-wrap justify-start"
      >
        <Text variant="caption" tone="muted">
          {t.host.computerAddressLabel}
        </Text>
        <Text variant="code" className="break-all">
          {props.lanIp ? addressText : t.host.unknownTransport}
        </Text>
        {props.copiedToast ? (
          <Text
            variant="caption"
            role="status"
            className="flex items-center gap-1"
          >
            <Check size={16} />
            {interpolate(t.host.addressCopied, { address: addressText })}
          </Text>
        ) : (
          <Copy size={16} />
        )}
      </Button>
      {privacyLabels && (
        <Text
          variant="caption"
          tone="muted"
          aria-label={t.host.privacySection}
          className="break-words"
        >
          {privacyLabels}
        </Text>
      )}
    </footer>
  );
}
export default DashboardFooter;
