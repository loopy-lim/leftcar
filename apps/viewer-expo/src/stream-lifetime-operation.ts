import { sameStreamLifetime } from "./stream-lifetime";
export { sameStreamLifetime, isStreamOperationCancelled } from "./stream-lifetime";
import type { ActiveStream } from "./catalog-model-types";
import type { StreamControlRequest } from "./launch-stream";
import { requestForCurrentSelection } from "./catalog-helpers";
import { captureRequestContext, isRequestContextCurrent } from "./session";

/** Snapshot the selection, allowing socket replacement within that selection. */
export function captureStreamSelection(host: string): () => boolean {
  const origin = captureRequestContext();
  return () => {
    const selected = captureRequestContext();
    return !!origin && !!selected && isRequestContextCurrent(selected) &&
      `${selected.target.host}:${selected.target.port}` === host &&
      selected.selectionGeneration === origin.selectionGeneration;
  };
}

interface StreamLifetimeOperation<T> {
  host: string;
  active: ActiveStream;
  getSnapshot: () => readonly ActiveStream[];
  work: (request: StreamControlRequest) => Promise<T>;
  commit: (result: T, current: ActiveStream) => void;
}

/** One user operation belongs to a Host selection and native window lifetime. */
export async function runStreamLifetimeOperation<T>(
  operation: StreamLifetimeOperation<T>,
): Promise<boolean> {
  const currentStream = () =>
    operation
      .getSnapshot()
      .find((current) => sameStreamLifetime(current, operation.active));
  if (!currentStream()) return false;
  const sameSelection = captureStreamSelection(operation.host);
  if (!sameSelection())
    throw new Error(
      "Host selection changed; this stream operation was cancelled",
    );
  // Capture before work can await metrics or native preparation.
  const selectedRequest = requestForCurrentSelection(operation.host);
  const request: StreamControlRequest = <R>(
    command: string,
    args?: unknown,
  ) => {
    if (!currentStream())
      return Promise.reject(
        new Error("Stream window changed; this operation was cancelled"),
      );
    return selectedRequest<R>(command, args);
  };
  const result = await operation.work(request);
  const current = currentStream();
  if (!current || !sameSelection()) return false;
  operation.commit(result, current);
  return true;
}
