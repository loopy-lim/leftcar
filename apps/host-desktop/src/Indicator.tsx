import { Text } from "./ui/primitives";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
} from "@leftcar/ui-tokens";
import { isTerminalSession } from "./streamTermination";
import type { SessionRow } from "./sessionTypes";

/**
 * "보고 있음" 표시 창(U4a). 스트리밍 중인 호스트의 캡처 화면에 트레이만
 * 있는 것은 no-stealth 규범에 어긋나므로, 항상 위 작은 배지로 연결 중임을
 * 드러낸다. 대시보드 창은 닫혀 있어도(숨겨져 있어도) 배지는 살아 있어야
 * 하므로 이 라우트가 get_status를 직접 폴링하고 자기 창을 show/hide 한다.
 */

function indicatorLanguage(): SupportedLanguage {
  const saved = localStorage.getItem("leftcar_lang");
  if (saved === "en") return "en";
  return "ko";
}

export default function Indicator() {
  const [connectedCount, setConnectedCount] = useState(0);
  const t = getTranslation(indicatorLanguage());

  useEffect(() => {
    const indicatorWindow = getCurrentWindow();
    let cancelled = false;
    const refresh = async () => {
      try {
        // 배지 스위치가 꺼져 있으면 세션 수와 무관하게 숨긴다 — 개인 기기
        // 조합의 기본값이며, 설정은 settings.json에 영속된다.
        const badgeEnabled = await invoke<boolean>("get_streaming_badge");
        if (cancelled) return;
        if (!badgeEnabled) {
          setConnectedCount(0);
          await indicatorWindow.hide();
          return;
        }
        const status = await invoke<{ sessions: SessionRow[] }>("get_status");
        if (cancelled) return;
        const active = (status.sessions ?? []).filter(
          (session) => !isTerminalSession(session),
        );
        setConnectedCount(active.length);
        if (active.length > 0) {
          await indicatorWindow.show();
        } else {
          await indicatorWindow.hide();
        }
      } catch {
        // 다음 폴링에서 다시 시도한다 — 배지 실패가 스트림을 죽이지 않는다.
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 2_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  if (connectedCount === 0) {
    // 숨김 상태(창 자체가 hide)지만 캡처에 찌꺼기가 남지 않도록 아무것도
    // 렌더링하지 않는다.
    return null;
  }

  return (
    <div className="flex h-dvh w-full select-none items-center gap-2 overflow-hidden rounded-md bg-curtain px-3 text-curtain-ink">
      <span
        aria-hidden="true"
        className="h-2 w-2 shrink-0 rounded-full bg-curtain-ink"
      />
      <Text
        variant="caption"
        className="whitespace-nowrap font-semibold text-curtain-ink"
      >
        {interpolate(t.host.indicatorStreaming, { count: connectedCount })}
      </Text>
    </div>
  );
}
