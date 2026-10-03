import type { TranslationSchema } from "@leftcar/ui-tokens";
import Modal from "./Modal";
import { DialogPanel } from "./modals/DialogPanel";
import { Button, Notice, Text } from "./ui/primitives";
export interface RevokeConfirm {
  deviceId: string | null;
  name: string | null;
}
interface RevokeConfirmDialogProps {
  confirm: RevokeConfirm;
  revoking: string | null;
  error?: string | null;
  t: TranslationSchema;
  onCancel: () => void;
  onConfirm: () => void;
}
export default function RevokeConfirmDialog({
  confirm,
  revoking,
  error,
  t,
  onCancel,
  onConfirm,
}: RevokeConfirmDialogProps) {
  const busy = revoking !== null;
  return (
    <Modal
      ariaLabel={t.host.revokeConfirmTitle}
      onClose={() => {
        if (!busy) onCancel();
      }}
    >
      <DialogPanel
        title={t.host.revokeConfirmTitle}
        closeLabel={t.common.close}
        onClose={onCancel}
        busy={busy}
        size="compact"
      >
        <div className="min-h-0 space-y-4 overflow-y-auto p-4">
          {confirm.name && (
            <Text className="block break-words font-semibold">
              {confirm.name}
            </Text>
          )}
          <p className="text-body text-muted">
            {confirm.deviceId
              ? t.host.revokeConfirmDesc
              : t.host.revokeConfirmAllDesc}
          </p>
          {error && <Notice tone="error">{error}</Notice>}
          <div className="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" disabled={busy} onClick={onCancel}>
              {t.common.cancel}
            </Button>
            <Button variant="danger" busy={busy} onClick={onConfirm}>
              {busy ? t.host.revoking : error ? t.common.retry : t.host.revoke}
            </Button>
          </div>
        </div>
      </DialogPanel>
    </Modal>
  );
}
