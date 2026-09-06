import type { ActiveStream } from "./catalog-model-types";

export type LocalStreamTerminationReason = 4 | 5;

export interface StreamTerminationEvent {
  port: number;
  reason: LocalStreamTerminationReason;
}

export interface StreamTerminationSubscription {
  remove(): void;
}

export interface RestartRequest {
  active: ActiveStream;
  trigger: "hostStatus" | "nativeTermination";
}

export type HostTerminationDisposition =
  | "viewerClosed"
  | "feedbackTimeout"
  | "hostStopped"
  | null;

export function classifyHostTermination(message: string): HostTerminationDisposition {
  if (message.trim().toLowerCase() === "viewer closed stream") return "viewerClosed";
  if (message.includes("feedback timeout")) return "feedbackTimeout";
  if (message.includes("host operator stopped")) return "hostStopped";
  return null;
}

function isLocalStreamTerminationEvent(
  event: unknown,
): event is StreamTerminationEvent {
  if (!event || typeof event !== "object") return false;
  const { port, reason } = event as Record<string, unknown>;
  return (
    typeof port === "number" &&
    Number.isFinite(port) &&
    Number.isInteger(port) &&
    (reason === 4 || reason === 5)
  );
}

export function selectRecoverableStream(
  streams: readonly ActiveStream[],
  event: unknown,
  inFlightSessions: ReadonlySet<number>,
): ActiveStream | null {
  if (!isLocalStreamTerminationEvent(event)) return null;
  const matching = streams.filter((stream) => stream.port === event.port);
  if (matching.length !== 1) return null;
  const active = matching[0];
  return inFlightSessions.has(active.session) ? null : active;
}

export function claimStreamRestore(
  inFlightSessions: Set<number>,
  session: number,
): boolean {
  if (inFlightSessions.has(session)) return false;
  inFlightSessions.add(session);
  return true;
}

export function releaseStreamRestore(
  inFlightSessions: Set<number>,
  session: number,
): void {
  inFlightSessions.delete(session);
}

export function subscribeStreamTermination(
  _listener: (event: StreamTerminationEvent) => void,
): StreamTerminationSubscription {
  return { remove: () => undefined };
}
