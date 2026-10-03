import type { ActiveStream } from "./catalog-model-types";
import type { InputEnableRequestEvent } from "./stream-input-request";
import { requestForCurrentSelection } from "./catalog-helpers";
import { captureRequestContext, isRequestContextCurrent } from "./session";
import { sameStreamLifetime } from "./stream-lifetime-operation";
import { formatErrorMessage } from "./control";

interface InputApprovalOperation {
  host: string;
  event: InputEnableRequestEvent;
  getSnapshot: () => readonly ActiveStream[];
  reportResult: (
    event: InputEnableRequestEvent,
    error: string | null,
  ) => Promise<void>;
}

export function requestStreamInputApproval(
  operation: InputApprovalOperation,
): Promise<void> {
  const { event } = operation;
  const active = operation
    .getSnapshot()
    .find((stream) => stream.port === event.port);
  const origin = captureRequestContext();
  if (
    !active ||
    !origin ||
    event.instanceId !== `src-${active.port}` ||
    !event.generation ||
    active.reservation?.generation !== event.generation
  )
    return Promise.resolve();
  const current = () => {
    const selection = captureRequestContext();
    return (
      !!selection &&
      isRequestContextCurrent(selection) &&
      selection.selectionGeneration === origin.selectionGeneration &&
      `${selection.target.host}:${selection.target.port}` === operation.host &&
      operation
        .getSnapshot()
        .some(
          (stream) =>
            sameStreamLifetime(stream, active) &&
            stream.reservation?.generation === event.generation,
        )
    );
  };
  if (!current()) return Promise.resolve();
  // Capture before the Host request can wait or reconnect.
  const request = requestForCurrentSelection(operation.host);
  const publish = (error: string | null) =>
    current()
      ? operation.reportResult(event, error).catch((cause) => {
          console.warn(
            "[leftcar] input request result window unavailable",
            cause,
          );
        })
      : Promise.resolve();
  return request("requestInputEnable", { session: active.session }).then(
    () => publish(null),
    (error) => publish(formatErrorMessage(error)),
  );
}
