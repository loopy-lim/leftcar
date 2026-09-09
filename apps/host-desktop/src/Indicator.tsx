import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getTranslation, interpolate, type SupportedLanguage } from "@leftcar/ui-tokens";
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
    <div
      style={{
        boxSizing: "border-box",
        width: "100%",
        height: "100vh",
        margin: 0,
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "0 10px",
        borderRadius: 8,
        background: "rgba(9, 9, 11, 0.92)",
        color: "#fafafa",
        fontFamily:
          "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Apple SD Gothic Neo', sans-serif",
        fontSize: 12,
        fontWeight: 700,
        whiteSpace: "nowrap",
        overflow: "hidden",
        userSelect: "none",
        WebkitUserSelect: "none",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 7,
          height: 7,
          borderRadius: "50%",
          background: "#f43f5e",
          boxShadow: "0 0 6px rgba(244, 63, 94, 0.9)",
          flexShrink: 0,
        }}
      />
      <span>{interpolate(t.host.indicatorStreaming, { count: connectedCount })}</span>
    </div>
  );
}
