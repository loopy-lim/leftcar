import type { ActiveStream } from "./catalog-model-types";

export function sameStreamLifetime(
  left: ActiveStream,
  right: ActiveStream,
): boolean {
  return (
    left.session === right.session &&
    left.port === right.port &&
    left.startedAt === right.startedAt &&
    left.reservation === right.reservation
  );
}

export function isStreamOperationCancelled(error: unknown): boolean {
  return error instanceof Error && (error.name === "AbortError" ||
    /(?:Host selection|Stream window) changed;.*cancelled/.test(error.message));
}
