import {
  isControlTransportError,
  type DisplayInfo,
} from "./control";
import { LocalizedError } from "./localized-error";
import { controlClient, reconnectHost } from "./session";
import { resolveStreamResolution } from "./stream-resolution";
import type { StreamProfile } from "./stream-profile";

const HIDABLE_DISPLAY_LABELS = ["leftcar hub", "leftcarhub"];

export function isHubDisplay(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return HIDABLE_DISPLAY_LABELS.some((label) => normalized.includes(label));
}

export function catalogDisplayHost(catalogHost: string): string {
  return catalogHost.split(":")[0] ?? "";
}

export function fitProfileToDisplay(
  display: DisplayInfo,
  profile: StreamProfile,
) {
  return resolveStreamResolution(display, profile);
}

export function catalogErrorMessage(error: unknown): string {
  if (error instanceof LocalizedError) return error.format();
  const message = String(error instanceof Error ? error.message : error);
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
  let client = controlClient();
  if (!client) throw new LocalizedError("errNotConnected");
  try {
    return await client.request<T>(command, args);
  } catch (error) {
    if (!isControlTransportError(error)) throw error;
    client = await reconnectHost();
    return client.request<T>(command, args);
  }
}
