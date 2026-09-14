import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getTranslation } from "@leftcar/ui-tokens";
import { createToggleGate, type ToggleGate } from "./toggleGate";

/**
 * 공통 낙관적 토글: 값을 즉시 뒤집어 반영하고, 호스트 명령이 실패하면
 * 되돌린다. settings.json 기반 게이트 토글이 모두 이 패턴을 쓴다.
 *
 * 요청이 진행 중인 동안에는 토글을 무시하고(버튼 비활성과 같은 효과),
 * superseded된 요청의 늦은 응답·실패는 최신 토글 상태를 덮지 않는다.
 * `pending`은 진행 중 요청이 있을 때 참이다.
 */
function useOptimisticToggle(command: string, initial: boolean): {
  value: boolean;
  pending: boolean;
  error: string | null;
  setError: (error: string | null) => void;
  toggle: () => void;
  setValue: (next: boolean) => void;
  gate: ToggleGate;
} {
  const [value, setValue] = useState(initial);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [gate] = useState(() => createToggleGate());
  const toggle = useCallback(() => {
    // 진행 중인 요청이 있으면 이번 토글을 무시한다 — 뒤섞인 요청 순서가
    // 최종 상태를 어기지 않게 한다.
    if (busy.current) return;
    busy.current = true;
    setError(null);
    const seq = gate.issue();
    const next = !value;
    setValue(next);
    setPending(true);
    void invoke(command, { enabled: next })
      .catch((cause: unknown) => {
        // superseded된 요청의 실패는 되돌리지 않는다 — 되돌리면 나중
        // 토글이 반영한 값을 덮어쓴다.
        if (gate.isCurrent(seq)) {
          setValue(!next);
          setError(String(cause instanceof Error ? cause.message : cause));
        }
      })
      .finally(() => {
        if (gate.isCurrent(seq)) {
          busy.current = false;
          setPending(false);
        }
      });
  }, [command, gate, value]);
  return { value, pending, error, setError, toggle, setValue, gate };
}

/**
 * 프라이버시 커튼의 대시보드 훅. 0600 settings.json에 살며 토글은 즉시
 * 효력을 가진다. 토글이 발행된 뒤에 늦게 도착한 초기 로드 응답은 무시한다.
 */
export function usePrivacySettings() {
  const [loadAttempt, retryLoad] = useState(0);
  const curtain = useOptimisticToggle("set_privacy_curtain", false);

  useEffect(() => {
    let cancelled = false;
    if (curtain.gate.allowsInitialLoad()) curtain.setError(null);
    void invoke<boolean>("get_privacy_settings")
      .then((curtainOn) => {
        if (cancelled) return;
        if (curtain.gate.allowsInitialLoad()) curtain.setValue(curtainOn);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        const message = String(cause instanceof Error ? cause.message : cause);
        if (curtain.gate.allowsInitialLoad()) curtain.setError(message);
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt, curtain.gate, curtain.setValue, curtain.setError]);

  return {
    privacyCurtain: curtain.value,
    togglePrivacyCurtain: curtain.toggle,
    pending: curtain.pending,
    curtainPending: curtain.pending,
    curtainError: curtain.error,
    retryCurtain: () => curtain.gate.allowsInitialLoad() ? retryLoad((attempt) => attempt + 1) : curtain.toggle(),
  };
}

/**
 * 클립보드 공유 호스트 게이트(U5)의 대시보드 훅. 0600 settings.json에서
 * 시작하며 토글은 즉시 효력을 가진다. 기본값은 꺼짐이다. 토글이 발행된
 * 뒤에 늦게 도착한 초기 로드 응답은 무시한다.
 */
export function useClipboardShare() {
  const [loadAttempt, retryLoad] = useState(0);
  const clipboard = useOptimisticToggle("set_clipboard_share", false);

  useEffect(() => {
    let cancelled = false;
    if (clipboard.gate.allowsInitialLoad()) clipboard.setError(null);
    void invoke<boolean>("get_clipboard_share")
      .then((enabled) => {
        if (!cancelled && clipboard.gate.allowsInitialLoad()) {
          clipboard.setValue(enabled);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled && clipboard.gate.allowsInitialLoad()) {
          clipboard.setError(String(cause instanceof Error ? cause.message : cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt, clipboard.gate, clipboard.setValue, clipboard.setError]);

  return {
    clipboardShare: clipboard.value,
    toggleClipboardShare: clipboard.toggle,
    pending: clipboard.pending,
    error: clipboard.error,
    retryClipboard: () => clipboard.gate.allowsInitialLoad() ? retryLoad((attempt) => attempt + 1) : clipboard.toggle(),
  };
}

/**
 * 프라이버시 커튼: 모니터를 채우는 검은 오버레이. 이 창은 macOS shim이
 * 캡처 필터에서 제외하므로(제목 "leftcar-curtain") 뷰어에게는 원래 화면이
 * 보인다. 창 자체는 아무 입력도 받지 않는 표시 전용이다.
 */
export function Curtain() {
  const t = getTranslation(
    localStorage.getItem("leftcar_lang") === "en" ? "en" : "ko",
  );
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "#000",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <span style={{ color: "#666", fontSize: 13 }}>{t.host.curtainHint}</span>
    </div>
  );
}
