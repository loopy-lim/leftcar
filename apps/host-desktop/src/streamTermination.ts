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

export function isTerminalSession(session: Pick<TerminationSession, "state">): boolean {
  return TERMINAL_STATES.has(session.state);
}

export function createTerminationNotice(
  session: TerminationSession,
  observedAt = new Date(),
): TerminationNotice {
  const error = session.error?.trim() ?? "";
  const normalized = error.toLowerCase();
  let title = "화면 공유가 종료되었습니다";
  let detail = "연결된 기기 또는 컴퓨터의 요청으로 화면 공유를 마쳤습니다.";
  let tone: TerminationTone = "neutral";

  if (normalized === "viewer closed stream") {
    title = "연결된 기기에서 화면 공유를 종료했습니다";
    detail = "연결된 기기에서 뒤로 가기 또는 닫기를 선택해 화면 공유를 마쳤습니다.";
  } else if (normalized.includes("operator stopped")) {
    title = "컴퓨터에서 화면 공유를 종료했습니다";
    detail = "영상 전송과 원격 조작을 중지하고 연결된 기기에 종료 사실을 알렸습니다.";
  } else if (normalized.includes("feedback timeout") || normalized.includes("connection lost")) {
    title = "연결된 기기의 응답이 없어 자동 종료했습니다";
    detail = "연결된 기기가 약 6초 동안 응답하지 않아 화면 공유를 안전하게 정리했습니다.";
    tone = "warning";
  } else if (normalized.includes("backend stats unavailable")) {
    title = "화면 상태를 확인할 수 없어 종료했습니다";
    detail = "화면 공유 상태를 확인할 수 없어 연결을 안전하게 정리했습니다.";
    tone = "danger";
  } else if (session.state === "error") {
    title = "문제가 생겨 화면 공유를 종료했습니다";
    detail = error || "화면을 가져오거나 보내는 중 문제가 발생했습니다.";
    tone = "danger";
  } else if (session.state === "unknown") {
    title = "화면 공유 상태를 확인할 수 없습니다";
    detail = error || "연결 상태를 확인할 수 없어 종료된 것으로 처리했습니다.";
    tone = "warning";
  } else if (error) {
    detail = error;
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
