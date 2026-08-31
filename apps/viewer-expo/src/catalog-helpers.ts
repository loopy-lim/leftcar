import {
  isControlTransportError,
  type DisplayInfo,
} from "./control";
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
  const message = String(error instanceof Error ? error.message : error);
  if (message.includes("SCShareableContent timed out")) {
    return "화면 소스 조회가 지연되고 있습니다. 잠시 후 새로고침을 눌러 주세요.";
  }
  if (message.includes("screen-recording permission")) {
    return "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 허용해 주세요.";
  }
  return message;
}

export async function requestWithReconnect<T>(
  command: string,
  args?: unknown,
): Promise<T> {
  let client = controlClient();
  if (!client) throw new Error("컴퓨터에 연결되어 있지 않습니다");
  try {
    return await client.request<T>(command, args);
  } catch (error) {
    if (!isControlTransportError(error)) throw error;
    client = await reconnectHost();
    return client.request<T>(command, args);
  }
}
