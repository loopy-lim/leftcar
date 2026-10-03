import { useCallback, useEffect, useRef, useState } from "react";
import { Text } from "./ui/primitives";
import { invoke } from "@tauri-apps/api/core";
import { getTranslation } from "@leftcar/ui-tokens";
import { createToggleGate } from "./toggleGate";

/** Settings are writable only after a successful read; UI reflects confirmed values. */
function useConfirmedToggle(
  getCommand: string,
  setCommand: string,
  initial: boolean,
) {
  const [value, setValue] = useState(initial);
  const [pending, setPending] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [loadAttempt, retryLoad] = useState(0);
  const ready = useRef(false);
  const busy = useRef(false);
  const mounted = useRef(false);
  const [gate] = useState(() => createToggleGate());

  useEffect(() => {
    mounted.current = true;
    setPending(true);
    setError(null);
    let cancelled = false;
    void invoke<boolean>(getCommand)
      .then((loaded) => {
        if (cancelled) return;
        setValue(loaded);
        ready.current = true;
      })
      .catch((cause: unknown) => {
        if (!cancelled)
          setError(String(cause instanceof Error ? cause.message : cause));
      })
      .finally(() => {
        if (!cancelled) setPending(false);
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [getCommand, loadAttempt]);

  const toggle = useCallback(() => {
    if (!ready.current || busy.current || !mounted.current) return;
    busy.current = true;
    setError(null);
    setPending(true);
    const seq = gate.issue();
    const next = !value;
    void invoke(setCommand, { enabled: next })
      .then(() => {
        if (mounted.current && gate.isCurrent(seq)) setValue(next);
      })
      .catch((cause: unknown) => {
        if (mounted.current && gate.isCurrent(seq)) {
          setError(String(cause instanceof Error ? cause.message : cause));
        }
      })
      .finally(() => {
        if (gate.isCurrent(seq)) {
          busy.current = false;
          if (mounted.current) setPending(false);
        }
      });
  }, [setCommand, gate, value]);

  const retry = () => {
    if (busy.current) return;
    if (ready.current) toggle();
    else retryLoad((attempt) => attempt + 1);
  };
  return { value, pending, error, toggle, retry, loaded: ready.current };
}

export function usePrivacySettings() {
  const curtain = useConfirmedToggle(
    "get_privacy_settings",
    "set_privacy_curtain",
    false,
  );
  return {
    privacyCurtain: curtain.value,
    curtainReady: curtain.loaded,
    togglePrivacyCurtain: curtain.toggle,
    pending: curtain.pending,
    curtainPending: curtain.pending,
    curtainError: curtain.error,
    retryCurtain: curtain.retry,
  };
}

export function useClipboardShare() {
  const clipboard = useConfirmedToggle(
    "get_clipboard_share",
    "set_clipboard_share",
    false,
  );
  return {
    clipboardShare: clipboard.value,
    ready: clipboard.loaded,
    toggleClipboardShare: clipboard.toggle,
    pending: clipboard.pending,
    error: clipboard.error,
    retryClipboard: clipboard.retry,
  };
}

export function useStreamingBadge() {
  const badge = useConfirmedToggle(
    "get_streaming_badge",
    "set_streaming_badge",
    false,
  );
  return {
    streamingBadge: badge.value,
    ready: badge.loaded,
    toggleStreamingBadge: badge.toggle,
    pending: badge.pending,
    error: badge.error,
    retryBadge: badge.retry,
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
  const [experiments, setExperiments] = useState<ViewerExperiments | null>(
    null,
  );
  const [error, setError] = useState<{
    operation: "load" | "save";
    detail: string;
  } | null>(null);
  const [phase, setPhase] = useState<"idle" | "saving">("idle");
  const saving = phase === "saving";
  const [loadAttempt, setLoadAttempt] = useState(0);
  const mounted = useRef(false);
  const busy = useRef(false);
  const lastSave = useRef<ViewerExperiments | null>(null);

  useEffect(() => {
    mounted.current = true;
    setError(null);
    let cancelled = false;
    void invoke<ViewerExperiments>("get_experiments")
      .then((loaded) => {
        if (!cancelled) setExperiments(loaded);
      })
      .catch((cause: unknown) => {
        if (!cancelled)
          setError({
            operation: "load",
            detail: String(cause instanceof Error ? cause.message : cause),
          });
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [loadAttempt]);

  const save = useCallback(async (next: ViewerExperiments) => {
    if (busy.current || !mounted.current) return;
    busy.current = true;
    lastSave.current = next;
    setPhase("saving");
    setError(null);
    try {
      const stored = await invoke<ViewerExperiments>("set_experiments", {
        experiment: next,
      });
      if (mounted.current) setExperiments(stored);
      lastSave.current = null;
    } catch (cause) {
      if (mounted.current)
        setError({
          operation: "save",
          detail: String(cause instanceof Error ? cause.message : cause),
        });
    } finally {
      busy.current = false;
      if (mounted.current) setPhase("idle");
    }
  }, []);

  const retry = () => {
    if (busy.current) return;
    if (lastSave.current) void save(lastSave.current);
    else setLoadAttempt((attempt) => attempt + 1);
  };
  return { experiments, error, saving, retry, save };
}

/** Preserve the saved WAN policy; never issue an inverse command from a placeholder. */
export function useWanAccess() {
  const wan = useConfirmedToggle("get_wan_access", "set_wan_access", true);
  return {
    wanAccess: wan.value,
    ready: wan.loaded,
    toggleWanAccess: wan.toggle,
    pending: wan.pending,
    error: wan.error,
    retryWan: wan.retry,
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
    <div className="fixed inset-0 flex items-center justify-center bg-curtain">
      <Text variant="caption" className="text-curtain-ink">
        {t.host.curtainHint}
      </Text>
    </div>
  );
}
