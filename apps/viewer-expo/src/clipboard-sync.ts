import * as ExpoClipboard from "expo-clipboard";
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import type { ControlClient } from "./control";

/**
 * 권한 게이트 클립보드 텍스트 동기화(U5, docs/07 §20 정책 변경).
 *
 * - 토글은 expo-secure-store의 `leftcar.clipboardShare`에 저장되고 기본은
 *   꺼짐이다. 호스트 쪽 게이트도 기본 꺼짐이므로 이중 잠금이다.
 * - 켜져 있고 제어 세션이 있을 때 2.5초마다 getClipboard를 마지막 해시로
 *   폴링한다. 호스트가 unchanged로 답하면 아무 것도 하지 않는다(짧은 폴링).
 * - 같은 루프에서 기기 클립보드도 읽어, 로컬 변경이 있고 그 해시가 호스트
 *   해시와 다르면 setClipboard으로 밀어 올린다.
 * - 루프 방지: 호스트는 자기 현재 내용과 같은 setClipboard을 무시한다.
 *   뷰어는 마지막으로 본 텍스트를 기억해 에코를 다시 밀지 않는다.
 */

export const CLIPBOARD_SHARE_KEY = "leftcar.clipboardShare";
export const CLIPBOARD_SYNC_INTERVAL_MS = 2_500;

export interface ClipboardSyncClient {
  request<T>(command: string, args?: unknown): Promise<T>;
}

/** expo-secure-store와 같은 모양의 문자열 저장소(테스트에서 대체한다). */
export interface ClipboardShareStore {
  getItemAsync(key: string): Promise<string | null>;
  setItemAsync(key: string, value: string): Promise<void>;
}

export interface ClipboardSyncIo {
  /** 현재 제어 세션. 없으면 폴링 라운드를 건너뛴다. */
  getClient(): ClipboardSyncClient | null;
  readDeviceClipboard(): Promise<string>;
  writeDeviceClipboard(text: string): Promise<void>;
}

/** expo-clipboard 기본 기기 입출력. 이 파일이 유일한 expo-clipboard 진입점이다. */
export const deviceClipboardIo: Pick<
  ClipboardSyncIo,
  "readDeviceClipboard" | "writeDeviceClipboard"
> = {
  readDeviceClipboard: async () => await ExpoClipboard.getStringAsync(),
  // setStringAsync는 boolean을 돌려주므로 void 계약으로 맞춘다.
  writeDeviceClipboard: async (text) => {
    await ExpoClipboard.setStringAsync(text);
  },
};

/** 폴링 루프의 진행 상태 — 마지막 해시와 마지막으로 관찰한 로컬 텍스트. */
export interface ClipboardSyncState {
  lastHash: string;
  localText: string;
}

export const INITIAL_CLIPBOARD_SYNC_STATE: ClipboardSyncState = {
  // 빈 텍스트의 sha256과 같다. 호스트 클립보드도 비어 있으면 첫 폴링이
  // unchanged로 끝나므로 초기 해시로 정확하다.
  lastHash:
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  localText: "",
};

export function clipboardHash(text: string): string {
  return bytesToHex(sha256(new TextEncoder().encode(text)));
}

/** 저장된 토글 값 해석. 값이 없거나 판독 실패면 기본 꺼짐이다. */
export function parseClipboardShare(raw: string | null): boolean {
  return raw === "1";
}

export async function loadClipboardShare(
  store: ClipboardShareStore,
): Promise<boolean> {
  try {
    return parseClipboardShare(await store.getItemAsync(CLIPBOARD_SHARE_KEY));
  } catch {
    return false;
  }
}

export async function saveClipboardShare(
  store: ClipboardShareStore,
  enabled: boolean,
): Promise<void> {
  await store.setItemAsync(CLIPBOARD_SHARE_KEY, enabled ? "1" : "0");
}

interface HostClipboardResult {
  unchanged?: boolean;
  text?: string;
  hash?: string;
}

/**
 * 폴링 라운드 한 번. 호스트가 새 텍스트를 주면 기기 클립보드에 쓰고 상태를
 * 갱신한다. unchanged면 상태 없이 그대로 돌려준다(해시 짧은 폴링).
 */
export async function pollHostClipboard(
  state: ClipboardSyncState,
  client: ClipboardSyncClient,
  io: ClipboardSyncIo,
): Promise<ClipboardSyncState> {
  let result: HostClipboardResult;
  try {
    result = await client.request<HostClipboardResult>("getClipboard", {
      hash: state.lastHash,
    });
  } catch {
    // 전송 오류·게이트 거부는 다음 폴링에서 다시 시도한다.
    return state;
  }
  if (result.unchanged || typeof result.text !== "string" || !result.hash) {
    return state;
  }
  try {
    await io.writeDeviceClipboard(result.text);
  } catch {
    // 쓰기 실패는 상태를 갱신하지 않는다 — 다음 폴링이 같은 텍스트로 재시도한다.
    return state;
  }
  return { lastHash: result.hash, localText: result.text };
}

/**
 * 기기 클립보드 변경 반영 한 번. 로컬 텍스트가 바뀌었고 그 해시가 호스트
 * 해시와 다를 때만 setClipboard으로 밀어 올린다.
 */
export async function pushDeviceClipboard(
  state: ClipboardSyncState,
  client: ClipboardSyncClient,
  io: ClipboardSyncIo,
): Promise<ClipboardSyncState> {
  let device: string;
  try {
    device = await io.readDeviceClipboard();
  } catch {
    return state;
  }
  if (device === state.localText) {
    // 우리가 방금 쓴 텍스트(호스트에서 내려온 에코)다 — 다시 올리지 않는다.
    return state;
  }
  const hash = clipboardHash(device);
  if (hash === state.lastHash) {
    return { ...state, localText: device };
  }
  try {
    await client.request("setClipboard", { text: device });
  } catch {
    return state;
  }
  return { lastHash: hash, localText: device };
}

/** 클립보드 동기화 루프 핸들. 토글을 끄면 타이머가 멈춘다. */
export interface ClipboardSyncLoop {
  setEnabled(enabled: boolean): void;
  isEnabled(): boolean;
  stop(): void;
}

/**
 * 폴링 루프 시작. getClipboard(내려받기)와 기기 클립보드 읽기(올리기)를
 * 같은 2.5초 주기로 처리한다. 제어 세션이 없으면 그 라운드는 건너뛴다.
 * 앱 화면당 한 번 시작하고 unmount에서 stop() 한다.
 */
export function startClipboardSync(io: ClipboardSyncIo): ClipboardSyncLoop {
  let enabled = false;
  let state: ClipboardSyncState = { ...INITIAL_CLIPBOARD_SYNC_STATE };
  let timer: ReturnType<typeof setInterval> | null = null;

  const tick = async () => {
    if (!enabled) return;
    const client = io.getClient();
    if (!client) return;
    state = await pollHostClipboard(state, client, io);
    state = await pushDeviceClipboard(state, client, io);
  };

  const loop: ClipboardSyncLoop = {
    setEnabled(next: boolean) {
      if (enabled === next) return;
      enabled = next;
      if (enabled) {
        timer = setInterval(() => void tick(), CLIPBOARD_SYNC_INTERVAL_MS);
      } else if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    },
    isEnabled: () => enabled,
    stop() {
      loop.setEnabled(false);
    },
  };
  return loop;
}
