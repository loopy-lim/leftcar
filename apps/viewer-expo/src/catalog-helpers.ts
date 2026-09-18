import { isControlTransportError } from "./control";
import { LocalizedError } from "./localized-error";
import {
  bindRequestContext,
  captureRequestContext,
  reconnectHost,
  isRequestContextCurrent,
} from "./session";
import type { StreamControlRequest } from "./launch-stream";

/** Pin each operation to the current selection of the window's owning Host.
 * A new operation after a catalog remount can use its new control connection;
 * an already pending operation must still stop if the selection changes. */
export function requestForCurrentSelection(expectedHost: string): StreamControlRequest {
  const selected = captureRequestContext();
  const origin = selected && `${selected.target.host}:${selected.target.port}` === expectedHost ? selected : null;
  return <T>(command: string, args?: unknown) => {
    const current = captureRequestContext();
    if (!origin || !current || !isRequestContextCurrent(current) ||
        current.selectionGeneration !== origin.selectionGeneration) {
      return Promise.reject(new Error("Host selection changed; this stream operation was cancelled"));
    }
    return requestWithReconnect<T>(command, args);
  };
}

const HIDABLE_DISPLAY_LABELS = ["leftcar hub", "leftcarhub"];

export function isHubDisplay(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return HIDABLE_DISPLAY_LABELS.some((label) => normalized.includes(label));
}

export function catalogDisplayHost(catalogHost: string): string {
  return catalogHost.split(":")[0] ?? "";
}

function isPrivateOrLocal(host: string): boolean {
  const normalized = host.toLowerCase().replace(/\.$/, "");
  if (normalized === "localhost" || normalized.endsWith(".local")) return true;
  const octets = normalized.split(".").map(Number);
  if (octets.length !== 4 || octets.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return false;
  }
  const [a, b] = octets;
  return (
    a === 127 ||
    a === 10 ||
    (a === 172 && b! >= 16 && b! <= 31) ||
    (a === 192 && b! === 168) ||
    (a === 169 && b! === 254)
  );
}

function isTailscale(host: string): boolean {
  const normalized = host.toLowerCase().replace(/\.$/, "");
  if (normalized.endsWith(".ts.net")) return true;
  const octets = normalized.split(".").map(Number);
  if (octets.length !== 4 || octets.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return false;
  }
  return octets[0] === 100 && octets[1]! >= 64 && octets[1]! <= 127;
}

export function catalogMediaHost(
  controlEndpoint: string,
  advertisedHost?: string | null,
  connectedPeer?: string,
  publicMediaEndpoint?: string | null,
): string {
  const selected = connectedPeer || catalogDisplayHost(controlEndpoint);
  const publicHost = publicMediaEndpoint ? catalogDisplayHost(publicMediaEndpoint) : undefined;

  // The advertised LAN address may be unreachable outside the Host's network.
  // Keep an explicitly selected tailnet route for both control and media.
  if (isTailscale(selected)) return selected;

  // For direct LAN/loopback connections, preserve the local route or advertised LAN host.
  if (isPrivateOrLocal(selected)) {
    return advertisedHost?.trim() || selected;
  }

  // For external/WAN connections, prefer the UPnP/NAT-mapped publicMediaEndpoint,
  // or preserve the selected public host. Never fall back to internal LAN advertisedHost.
  return publicHost?.trim() || selected;
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
  if (!isRequestContextCurrent(origin)) throw new Error("Host selection changed; this stream operation was cancelled");
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
