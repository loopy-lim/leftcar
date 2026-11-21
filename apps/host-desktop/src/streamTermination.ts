import type { SupportedLanguage } from "@leftcar/ui-tokens";

export interface TerminationSession {
  session: number;
  sourceName: string;
  viewerAddr: string;
  state: string;
  error?: string | null;
}

export type TerminationTone = "neutral" | "warning" | "danger";

export interface TerminationNotice {
  key: string;
  session: number;
  sourceName: string;
  viewerAddr: string;
  title: string;
  detail: string;
  tone: TerminationTone;
  observedAt: Date;
}

const TERMINAL_STATES = new Set(["error", "stopped", "unknown"]);

/** 안내문을 우선 표시하고 영어 원문은 괄호 뒤에 붙여 진단 가능성을 유지한다. */
function withRawDetail(raw: string, fallback: string): string {
  return raw ? `${fallback} (${raw})` : fallback;
}

export function isTerminalSession(session: Pick<TerminationSession, "state">): boolean {
  return TERMINAL_STATES.has(session.state);
}

export function createTerminationNotice(
  session: TerminationSession,
  observedAt = new Date(),
  language: SupportedLanguage = "ko",
): TerminationNotice {
  const error = session.error?.trim() ?? "";
  const normalized = error.toLowerCase();
  const isEn = language === "en";
  let title = isEn ? "Screen sharing ended" : "화면 공유가 종료되었습니다";
  let detail = isEn
    ? "Screen sharing ended at the request of the connected device or computer."
    : "연결된 기기 또는 컴퓨터의 요청으로 화면 공유를 마쳤습니다.";
  let tone: TerminationTone = "neutral";

  if (normalized === "viewer closed stream") {
    title = isEn
      ? "Screen sharing was stopped from the connected device"
      : "연결된 기기에서 화면 공유를 종료했습니다";
    detail = isEn
      ? "The connected device chose Back or Close to end screen sharing."
      : "연결된 기기에서 뒤로 가기 또는 닫기를 선택해 화면 공유를 마쳤습니다.";
  } else if (normalized.includes("operator stopped")) {
    title = isEn
      ? "Screen sharing was stopped on this computer"
      : "컴퓨터에서 화면 공유를 종료했습니다";
    detail = isEn
      ? "Video streaming and remote input stopped, and the connected device was notified."
      : "영상 전송과 원격 조작을 중지하고 연결된 기기에 종료 사실을 알렸습니다.";
  } else if (normalized.includes("feedback timeout") || normalized.includes("connection lost")) {
    title = isEn
      ? "Automatically stopped because the device stopped responding"
      : "연결된 기기의 응답이 없어 자동 종료했습니다";
    detail = isEn
      ? "The connected device did not respond for about 6 seconds, so screen sharing was safely closed."
      : "연결된 기기가 약 6초 동안 응답하지 않아 화면 공유를 안전하게 정리했습니다.";
    tone = "warning";
  } else if (normalized.includes("backend stats unavailable")) {
    title = isEn
      ? "Stopped because display status could not be checked"
      : "화면 상태를 확인할 수 없어 종료했습니다";
    detail = isEn
      ? "The stream was safely closed because screen sharing status became unavailable."
      : "화면 공유 상태를 확인할 수 없어 연결을 안전하게 정리했습니다.";
    tone = "danger";
  } else if (normalized.includes("screen recording") || normalized.includes("screen-recording")) {
    title = isEn
      ? "Stopped due to a screen recording permission issue"
      : "화면 공유 권한 문제로 종료했습니다";
    detail = isEn
      ? "Screen Recording permission is turned off on this computer. Allow Leftcar Host in System Settings."
      : "컴퓨터의 화면 녹화 권한이 꺼져 있습니다. 시스템 설정에서 Leftcar Host를 허용해 주세요.";
    tone = "danger";
  } else if (session.state === "error") {
    title = isEn
      ? "Screen sharing stopped due to an error"
      : "문제가 생겨 화면 공유를 종료했습니다";
    detail = withRawDetail(
      error,
      isEn
        ? "An error occurred while capturing or sending the screen."
        : "화면을 가져오거나 보내는 중 문제가 발생했습니다.",
    );
    tone = "danger";
  } else if (session.state === "unknown") {
    title = isEn
      ? "Screen sharing status is unknown"
      : "화면 공유 상태를 확인할 수 없습니다";
    detail = withRawDetail(
      error,
      isEn
        ? "The stream was treated as stopped because its status could not be verified."
        : "연결 상태를 확인할 수 없어 종료된 것으로 처리했습니다.",
    );
    tone = "warning";
  } else if (error) {
    detail = withRawDetail(
      error,
      isEn
        ? "Screen sharing ended with an issue."
        : "화면 공유가 문제로 종료되었습니다.",
    );
    tone = "warning";
  }

  return {
    key: `${session.session}:${session.state}:${error}`,
    session: session.session,
    sourceName: session.sourceName,
    viewerAddr: session.viewerAddr,
    title,
    detail,
    tone,
    observedAt,
  };
}
