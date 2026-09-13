import { X } from "lucide-react";
import { buttonVariants } from "./lib/variants";
import Modal from "./Modal";
import type { TranslationSchema } from "@leftcar/ui-tokens";

/** 확인 다이얼로그가 기다리는 삭제 요청 — 단일 기기(deviceId) 또는 전체(null). */
export interface RevokeConfirm {
  deviceId: string | null;
  name: string | null;
}

interface RevokeConfirmDialogProps {
  confirm: RevokeConfirm;
  revoking: string | null;
  t: TranslationSchema;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * 기기 연결 삭제 확인 — 보안상 되돌릴 수 없는 동작이므로 실행 전 한 번
 * 묻는다. 대상 기기 이름과 함께 영향 범위를 한 줄로 보여 준다.
 */
export default function RevokeConfirmDialog({
  confirm,
  revoking,
  t,
  onCancel,
  onConfirm,
}: RevokeConfirmDialogProps) {
  return (
    <Modal ariaLabel={t.host.revokeConfirmTitle} onClose={onCancel}>
      <div
        className="modal-window"
        onClick={(event) => event.stopPropagation()}
        style={{ maxWidth: 360 }}
      >
        <div className="modal-title-bar">
          <h3>{t.host.revokeConfirmTitle}</h3>
          <button
            className={buttonVariants({ variant: "close" })}
            onClick={onCancel}
            aria-label={t.common.close}
          >
            <X size={15} />
          </button>
        </div>
        <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 14 }}>
          <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--text-secondary)" }}>
            {confirm.name ? <strong>{confirm.name}</strong> : null}
            {confirm.name ? " · " : ""}
            {confirm.deviceId ? t.host.revokeConfirmDesc : t.host.revokeConfirmAllDesc}
          </p>
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button className={buttonVariants({ variant: "ghost", size: "sm" })} onClick={onCancel}>
              {t.common.cancel}
            </button>
            <button className="btn-danger-outline btn-sm" disabled={revoking !== null} onClick={onConfirm}>
              {t.host.revoke}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
