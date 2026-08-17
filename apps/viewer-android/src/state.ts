/**
 * Viewer UI state model (docs/01 §6) mapped to StreamPhase names.
 * Pure logic, unit-tested without React Native.
 */

export type StreamPhase =
  | "idle"
  | "negotiating"
  | "waiting_keyframe"
  | "playing"
  | "degraded"
  | "reconnecting"
  | "suspended"
  | "source_unavailable"
  | "permission_revoked"
  | "decoder_failed"
  | "stopped";

export type PairingState =
  | "unpaired"
  | "advertising"
  | "awaiting_host_approval"
  | "paired_offline"
  | "connecting"
  | "connected"
  | "revoked";

/** Terminal-any states (docs/01 §6). */
const ANY_REACHABLE: ReadonlySet<StreamPhase> = new Set([
  "source_unavailable",
  "permission_revoked",
  "decoder_failed",
  "stopped",
]);

const EDGES: Readonly<Record<string, StreamPhase[]>> = {
  idle: ["negotiating", "stopped"],
  negotiating: ["waiting_keyframe", "stopped"],
  waiting_keyframe: ["playing", "stopped"],
  playing: ["degraded", "reconnecting", "suspended", "stopped"],
  degraded: ["playing", "reconnecting", "suspended", "stopped"],
  reconnecting: ["playing", "stopped"],
  suspended: ["negotiating", "stopped"],
};

/** May `from` transition to `to`? Mirrors domain::StreamPhase. */
export function canTransition(from: StreamPhase, to: StreamPhase): boolean {
  if (to === from) return true;
  if (ANY_REACHABLE.has(to)) return true;
  return (EDGES[from] ?? []).includes(to);
}

/** User-facing status line per phase (H38: no raw error codes). */
export function statusLine(phase: StreamPhase): { label: string; retryable: boolean } {
  switch (phase) {
    case "idle":
      return { label: "대기 중", retryable: false };
    case "negotiating":
      return { label: "연결 준비 중", retryable: false };
    case "waiting_keyframe":
      return { label: "첫 화면을 기다리는 중", retryable: false };
    case "playing":
      return { label: "재생 중", retryable: false };
    case "degraded":
      return { label: "화질이 낮아진 상태로 재생 중", retryable: true };
    case "reconnecting":
      return { label: "다시 연결하는 중", retryable: true };
    case "suspended":
      return { label: "일시 중지됨", retryable: true };
    case "source_unavailable":
      return { label: "원본 창을 더 이상 사용할 수 없어요. 호스트에서 소스를 다시 선택해 주세요.", retryable: true };
    case "permission_revoked":
      return { label: "화면 녹화 권한이 없어요. 시스템 설정에서 허용해 주세요.", retryable: true };
    case "decoder_failed":
      return { label: "영상을 표시할 수 없어요. 창을 닫고 다시 열어 주세요.", retryable: true };
    case "stopped":
      return { label: "중지됨", retryable: false };
  }
}

/** Launch-handle policy: handles are opaque and expire (docs/04 §6.2). */
export function parseLaunchHandle(handle: string): { valid: boolean; sourceId?: string } {
  if (!handle.startsWith("leftcar-launch://")) return { valid: false };
  const rest = handle.slice("leftcar-launch://".length);
  const sourceId = rest.split("/")[0];
  if (!sourceId) return { valid: false };
  return { valid: true, sourceId };
}
