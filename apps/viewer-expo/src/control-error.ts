import { LocalizedError } from "./localized-error";

export type ControlErrorKind = "remote" | "timeout" | "transport" | "unauthorized";

export class ControlRequestError extends Error {
  constructor(
    message: string,
    readonly kind: ControlErrorKind,
  ) {
    super(message);
    this.name = "ControlRequestError";
  }
}

/** A transport error cannot prove a mutating command failed on the Host. */
export class AmbiguousControlError extends LocalizedError {
  constructor(readonly command: string, readonly cause: unknown) {
    super("errOperationUncertain");
    this.name = "AmbiguousControlError";
  }
}

/** A verified remote response proves the previous session is already absent. */
export function isAbsentControlSession(error: unknown): boolean {
  return error instanceof ControlRequestError && error.kind === "remote" &&
    /^no such session(?: \d+)?$/.test(error.message);
}
