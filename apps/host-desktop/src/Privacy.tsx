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
 * 프라이버시 옵션(종료 시 잠금 · 커튼)의 대시보드 훅. 같은 0600
 * settings.json에 살며 토글은 즉시 효력을 가진다. get_privacy_settings는
 * [잠금, 커튼] 순서의 튜플을 돌려준다. 토글이 발행된 뒤에 늦게 도착한
 * 초기 로드 응답은 무시한다.
 */
export function usePrivacySettings() {
  const [loadAttempt, retryLoad] = useState(0);
  const lock = useOptimisticToggle("set_lock_on_disconnect", false);
  const curtain = useOptimisticToggle("set_privacy_curtain", false);

  useEffect(() => {
    let cancelled = false;
    if (lock.gate.allowsInitialLoad()) lock.setError(null);
    if (curtain.gate.allowsInitialLoad()) curtain.setError(null);
    void invoke<[boolean, boolean]>("get_privacy_settings")
      .then(([lockOn, curtainOn]) => {
        if (cancelled) return;
        if (lock.gate.allowsInitialLoad()) lock.setValue(lockOn);
        if (curtain.gate.allowsInitialLoad()) curtain.setValue(curtainOn);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        const message = String(cause instanceof Error ? cause.message : cause);
        if (lock.gate.allowsInitialLoad()) lock.setError(message);
        if (curtain.gate.allowsInitialLoad()) curtain.setError(message);
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt, lock.gate, lock.setValue, lock.setError, curtain.gate, curtain.setValue, curtain.setError]);

  return {
    lockOnDisconnect: lock.value,
    privacyCurtain: curtain.value,
    toggleLockOnDisconnect: lock.toggle,
    togglePrivacyCurtain: curtain.toggle,
    pending: lock.pending || curtain.pending,
    lockPending: lock.pending,
    curtainPending: curtain.pending,
    lockError: lock.error,
    curtainError: curtain.error,
    retryLock: () => lock.gate.allowsInitialLoad() ? retryLoad((attempt) => attempt + 1) : lock.toggle(),
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
 * 스트리밍 배지("N대 연결 중" 표시) 호스트 훅. 개인 기기 조합에서는
 * 소음이므로 기본 꺼짐이며, settings.json에 영속된다. Indicator 라우트가
 * 같은 값을 폴링해 배지 창의 show/hide를 따른다.
 */
export function useStreamingBadge() {
  const [loadAttempt, retryLoad] = useState(0);
  const badge = useOptimisticToggle("set_streaming_badge", false);

  useEffect(() => {
    let cancelled = false;
    if (badge.gate.allowsInitialLoad()) badge.setError(null);
    void invoke<boolean>("get_streaming_badge")
      .then((enabled) => {
        if (!cancelled && badge.gate.allowsInitialLoad()) {
          badge.setValue(enabled);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled && badge.gate.allowsInitialLoad()) {
          badge.setError(String(cause instanceof Error ? cause.message : cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt, badge.gate, badge.setValue, badge.setError]);

  return {
    streamingBadge: badge.value,
    toggleStreamingBadge: badge.toggle,
    pending: badge.pending,
    error: badge.error,
    retryBadge: () => badge.gate.allowsInitialLoad() ? retryLoad((attempt) => attempt + 1) : badge.toggle(),
  };
}

/**
 * 실험 스위치(페이싱 A/B 노브) 훅. 설정 모달의 실험 섹션이 쓴다. 저장은
 * 전체 객체 하나로 보내고(부분 갱신 경로 없음 — 마지막 저장이 이긴다), 호스트는
 * 저장과 함께 프로세스 환경변수를 다시 심으므로 다음 스트림부터 적용된다.
 * `null`은 미설정(shim 기본값)이다.
 */
export interface ViewerExperiments {
  maxEncodeInFlight: number | null;
  queueMaxAgeMs: number | null;
  sndbufBytes: number | null;
  drlWindowMs: number | null;
  pacingBudgetPct: number | null;
  frameTrace: boolean;
}

export function useExperiments() {
  const [experiments, setExperiments] = useState<ViewerExperiments | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void invoke<ViewerExperiments>("get_experiments")
      .then((loaded) => {
        if (!cancelled) setExperiments(loaded);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(String(cause instanceof Error ? cause.message : cause));
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt]);

  const save = useCallback(async (next: ViewerExperiments) => {
    setSaving(true);
    setError(null);
    try {
      const stored = await invoke<ViewerExperiments>("set_experiments", {
        experiment: next,
      });
      setExperiments(stored);
    } catch (cause: unknown) {
      setError(String(cause instanceof Error ? cause.message : cause));
    } finally {
      setSaving(false);
    }
  }, []);

  return {
    experiments,
    error,
    saving,
    retry: () => setLoadAttempt((attempt) => attempt + 1),
    save,
  };
}

/**
 * 외부 접속(WAN) 허용 훅. UPnP 포트 매핑의 등록·제거는 호스트 명령이
 * 백그라운드에서 즉시 실행하며, 값은 같은 0600 settings.json에 영속된다.
 * 소유자 결정(2026-09-19)에 따라 기본 켜짐이다.
 */
export function useWanAccess() {
  const [loadAttempt, retryLoad] = useState(0);
  const wan = useOptimisticToggle("set_wan_access", true);

  useEffect(() => {
    let cancelled = false;
    if (wan.gate.allowsInitialLoad()) wan.setError(null);
    void invoke<boolean>("get_wan_access")
      .then((enabled) => {
        if (!cancelled && wan.gate.allowsInitialLoad()) {
          wan.setValue(enabled);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled && wan.gate.allowsInitialLoad()) {
          wan.setError(String(cause instanceof Error ? cause.message : cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt, wan.gate, wan.setValue, wan.setError]);

  return {
    wanAccess: wan.value,
    toggleWanAccess: wan.toggle,
    pending: wan.pending,
    error: wan.error,
    retryWan: () => wan.gate.allowsInitialLoad() ? retryLoad((attempt) => attempt + 1) : wan.toggle(),
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
