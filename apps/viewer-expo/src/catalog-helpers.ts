import { isControlTransportError } from "./control";
import { LocalizedError } from "./localized-error";
import {
  bindRequestContext,
  captureRequestContext,
  reconnectHost,
} from "./session";

const HIDABLE_DISPLAY_LABELS = ["leftcar hub", "leftcarhub"];

export function isHubDisplay(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return HIDABLE_DISPLAY_LABELS.some((label) => normalized.includes(label));
}

export function catalogDisplayHost(catalogHost: string): string {
  return catalogHost.split(":")[0] ?? "";
}

export function catalogErrorMessage(error: unknown): string {
  if (error instanceof LocalizedError) return error.format();
  const message = String(error instanceof Error ? error.message : error);
  if (/source_(access_denied|refresh_required|unavailable)/.test(message)) {
    return new LocalizedError("errSourceAccess").format();
  }
  if (message.includes("SCShareableContent timed out")) {
    return new LocalizedError("errCatalogSourceSlow").format();
  }
  if (message.includes("screen-recording permission")) {
    return new LocalizedError("errCatalogScreenPermission").format();
  }
  return message;
}

export async function requestWithReconnect<T>(
  command: string,
  args?: unknown,
): Promise<T> {
  const origin = captureRequestContext();
  if (!origin) throw new LocalizedError("errNotConnected");
  try {
    return await origin.client.request<T>(command, args);
  } catch (error) {
    bindRequestContext(error, origin);
    if (!isControlTransportError(error)) throw error;
    const client = await reconnectHost(origin);
    const retry = captureRequestContext();
    try {
      return await client.request<T>(command, args);
    } catch (retryError) {
      if (retry) bindRequestContext(retryError, retry);
      throw retryError;
    }
  }
}
