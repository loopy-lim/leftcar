import { useEffect, useRef, type ReactNode } from "react";

interface ModalProps {
  /** 접근성 이름 — 제목 h3에 붙일 id(aria-labelledby) 또는 직접 라벨(aria-label). */
  labelledBy?: string;
  ariaLabel?: string;
  onClose: () => void;
  /** 오버레이(배경) 클릭으로 닫는다 — 원래 click-outside 닫기가 있던 모달만 켠다. */
  closeOnOverlayClick?: boolean;
  children: ReactNode;
}

/**
 * 공통 모달 — 네이티브 `<dialog>`를 showModal()로 연다. 브라우저가 Esc
 * 취소(cancel 이벤트), 포커스 가두기, 배경 inert를 맡아 주므로 모달마다
 * 키 핸들러를 둘 필요가 없다. 열릴 때 포커스는 첫 포커스 가능한 요소(제목
 * 행의 닫기 버튼)로 이동하고, 닫히면 열기 전 포커스를 되돌린다.
 */
export default function Modal({
  labelledBy,
  ariaLabel,
  onClose,
  closeOnOverlayClick = false,
  children,
}: ModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const previouslyFocused = useRef<Element | null>(null);
  // onClose는 호출부에서 매번 새 화살표라 구독 효과의 의존성에서 뺀다 —
  // 항상 최신 콜백만 보면 되므로 재구독도 필요 없다.
  const onCloseRef = useRef(onClose);
  useEffect(() => {
    onCloseRef.current = onClose;
  });

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    previouslyFocused.current = document.activeElement;
    dialog.showModal();
    return () => {
      dialog.close();
      if (previouslyFocused.current instanceof HTMLElement) {
        previouslyFocused.current.focus();
      }
    };
  }, []);

  // 오버레이 클릭은 네이티브 dialog에 직접 듣는다 — 클릭이 창 안에서 끝나면
  // target이 dialog가 아니므로 닫히지 않는다. 키보드 닫기는 Esc(cancel 이벤트)
  // 이 담당한다.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog || !closeOnOverlayClick) return;
    const handleOverlayClick = (event: MouseEvent) => {
      if (event.target === dialog) onCloseRef.current();
    };
    dialog.addEventListener("click", handleOverlayClick);
    return () => dialog.removeEventListener("click", handleOverlayClick);
  }, [closeOnOverlayClick]);

  return (
    <dialog
      ref={dialogRef}
      className="modal-overlay"
      aria-labelledby={labelledBy}
      aria-label={ariaLabel}
      onCancel={(event) => {
        // 브라우저 자체 닫힘을 막고 닫기 여부를 React 상태에 남긴다 — busy
        // 중이라 닫으면 안 되는 모달(종료 확인)은 onClose에서 골라 낸다.
        event.preventDefault();
        onClose();
      }}
    >
      {children}
    </dialog>
  );
}
