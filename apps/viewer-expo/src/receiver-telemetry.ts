import type { SessionView } from "./control";

/**
 * Receiver feedback older than this is a stopped backgrounded viewer, not a
 * live measurement; its last rendered FPS value must not drive policy.
 */
export const RENDERED_FPS_STALE_MS = 5_000;

type RenderedFpsSession = Pick<
  SessionView,
  | "renderedFps"
  | "receiverFeedbackAgeMs"
  | "splitDirection"
  | "leftRenderedFps"
  | "rightRenderedFps"
  | "joinedRenderedFps"
>;

/** A sample only counts when its feedback age is known, sane, and in budget. */
function hasFreshFeedback(age: SessionView["receiverFeedbackAgeMs"]): boolean {
  return typeof age === "number" && Number.isFinite(age) && age >= 0 &&
    age <= RENDERED_FPS_STALE_MS;
}

function isKnownFps(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

/**
 * Fresh receiver-measured output frame rate, or undefined when no trustworthy
 * sample exists. Feedback must carry a known, finite, non-negative
 * `receiverFeedbackAgeMs` inside the staleness budget — missing/null/NaN/
 * negative/over-budget ages map to undefined, never to a guess.
 *
 * A frozen receiver legitimately reports 0. In split mode the Host maps its
 * aggregate 0 to null (`renderedFps` 0→nil), which would hide a frozen
 * receiver — so a split session reports the raw `joinedRenderedFps`, or the
 * valid minimum of both decoders when the joined value is absent. Outside
 * split mode the aggregate `renderedFps` is used as-is and a fresh 0 stays 0.
 * An absent split reading is never invented into a default 0.
 */
export function receiverRenderedFps(
  session: RenderedFpsSession,
): number | undefined {
  if (!hasFreshFeedback(session.receiverFeedbackAgeMs)) return undefined;
  const splitMode = typeof session.splitDirection === "string" &&
    session.splitDirection.length > 0;
  if (splitMode) {
    const joined = session.joinedRenderedFps;
    if (isKnownFps(joined)) return joined;
    const left = session.leftRenderedFps;
    const right = session.rightRenderedFps;
    if (isKnownFps(left) && isKnownFps(right)) return Math.min(left, right);
    return undefined;
  }
  const fps = session.renderedFps;
  return isKnownFps(fps) ? fps : undefined;
}

/**
 * Oldest pending age across the Host-side queues: the single-encoder encode
 * pending queue, the split encoded queue, and the split capture queue
 * (pending capture age). Missing metrics contribute nothing — unknown stays
 * unknown instead of pretending to be zero pressure elsewhere.
 */
export function hostQueuePressureUs(
  session: Pick<
    SessionView,
    | "pendingFrameOldestAgeUs"
    | "splitEncodedQueueOldestUs"
    | "splitCaptureQueueOldestUs"
  >,
): number {
  return Math.max(
    session.pendingFrameOldestAgeUs ?? 0,
    session.splitEncodedQueueOldestUs ?? 0,
    session.splitCaptureQueueOldestUs ?? 0,
  );
}
