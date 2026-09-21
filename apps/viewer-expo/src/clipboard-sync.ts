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
 * - 호스트 게이트가 꺼져 "clipboard share disabled"로 거부되면 연속
 *   [CLIPBOARD_GATE_LATCH_LIMIT]회까지 시도한 뒤 푸시를 멈춘다 — 설정을
 *   다시 토글하면 재시도한다. 진행 중 라운드가 겹치면 다음 틱은 건너뛴다.
 */

export const CLIPBOARD_SHARE_KEY = "leftcar.clipboardShare";
export const CLIPBOARD_SYNC_INTERVAL_MS = 2_500;

/**
 * 호스트 게이트 꺼짐 래치 한도 — setClipboard이 이 횟수만큼 연속으로
 * "clipboard share disabled"로 거부되면 설정 변경까지 푸시를 멈춘다.
 */
export const CLIPBOARD_GATE_LATCH_LIMIT = 3;

/** 호스트가 클립보드 게이트를 꺼 두었을 때 답하는 오류 문자열(control.rs). */
export function isClipboardGateError(error: unknown): boolean {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";
  return raw.includes("clipboard share disabled");
}

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
  /** Optional native change signal. Returning null retains bounded polling. */
  subscribeDeviceChanges?(listener: () => void): (() => void) | null;
  readDeviceClipboard(): Promise<string>;
  writeDeviceClipboard(text: string): Promise<void>;
  /** 기기 클립보드의 이미지(PNG base64). 없으면 null. */
  readDeviceClipboardImage(): Promise<string | null>;
  writeDeviceClipboardImage(base64: string): Promise<void>;
}

/** expo-clipboard 기본 기기 입출력. 이 파일이 유일한 expo-clipboard 진입점이다. */
export const deviceClipboardIo: Pick<
  ClipboardSyncIo,
  "readDeviceClipboard" | "writeDeviceClipboard" | "readDeviceClipboardImage" | "writeDeviceClipboardImage" | "subscribeDeviceChanges"
> = {
  subscribeDeviceChanges: (listener) => {
    if (typeof ExpoClipboard.addClipboardListener !== "function") return null;
    const subscription = ExpoClipboard.addClipboardListener(listener);
    return () => subscription.remove();
  },
  readDeviceClipboard: async () => await ExpoClipboard.getStringAsync(),
  // setStringAsync는 boolean을 돌려주므로 void 계약으로 맞춘다.
  writeDeviceClipboard: async (text) => {
    await ExpoClipboard.setStringAsync(text);
  },
  readDeviceClipboardImage: async () => {
    const image = await ExpoClipboard.getImageAsync({ format: "png" });
    return image?.data ?? null;
  },
  writeDeviceClipboardImage: async (base64) => {
    await ExpoClipboard.setImageAsync(base64);
  },
};

/** 폴링 루프의 진행 상태 — 마지막 해시와 마지막으로 관찰한 로컬 내용. */
export interface ClipboardSyncState {
  lastHash: string;
  localText: string;
  /** 호스트에서 내려와 기기에 쓴 마지막 이미지("i:"+sha256(base64)) — 에코 방지. */
  localImageHash: string;
  /** 연속된 호스트 게이트 꺼짐 거부 횟수 — 임계치에 닿으면 푸시를 멈춘다. */
  gateRejections: number;
}

export const INITIAL_CLIPBOARD_SYNC_STATE: ClipboardSyncState = {
  // 빈 텍스트의 sha256과 같다. 호스트 클립보드도 비어 있으면 첫 폴링이
  // unchanged로 끝나므로 초기 해시로 정확하다.
  lastHash:
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  localText: "",
  localImageHash: "",
  gateRejections: 0,
};

/** 게이트 꺼짐 거부 한 번 — 연속 횟수만 올린다(성공 시 0으로 재무장). */
function gateRejected(state: ClipboardSyncState): ClipboardSyncState {
  return { ...state, gateRejections: state.gateRejections + 1 };
}

/** 이미지 클립보드의 동기화 해시 — 호스트 getClipboard와 같은 규칙. */
export function imageClipboardHash(base64: string): string {
  return `i:${clipboardHash(base64)}`;
}

export function clipboardHash(text: string): string {
  return bytesToHex(sha256(new TextEncoder().encode(text)));
}

// 취소는 pairing.ts와 같은 AbortSignal 관례를 쓴다 — 토글을 끄거나 루프를
// 멈추면 진행 중 라운드는 이미 시작한 패킷을 되돌릴 수는 없지만, 그 뒤의
// 읽기·쓰기·전송은 시작하지 않는다.
function abortError(): Error {
  const error = new Error("clipboard sync round aborted");
  error.name = "AbortError";
  return error;
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw abortError();
}

function assertRoundActive(signal?: AbortSignal, isCurrent?: () => boolean): void {
  throwIfAborted(signal);
  if (isCurrent && !isCurrent()) throw abortError();
}

/** 저장된 토글 값 해석. 값이 없으면 기본 꺼짐이다. */
export function parseClipboardShare(raw: string | null): boolean {
  return raw === "1";
}

export async function loadClipboardShare(
  store: ClipboardShareStore,
): Promise<boolean> {
  const raw = await store.getItemAsync(CLIPBOARD_SHARE_KEY);
  if (raw !== null && raw !== "0" && raw !== "1") {
    throw new Error("Invalid clipboard preference");
  }
  return parseClipboardShare(raw);
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
  /** 이미지 클립보드(텍스트가 비어 있을 때만). PNG base64. */
  imageBase64?: string;
  hash?: string;
}

/**
 * 폴링 라운드 한 번. 호스트가 새 텍스트를 주면 기기 클립보드에 쓰고 상태를
 * 갱신한다. unchanged면 상태 없이 그대로 돌려준다(해시 짧은 폴링).
 * `signal`이 이미 끈 취소면 기기 쓰기를 시작하지 않고 AbortError로 끝난다.
 */
export async function pollHostClipboard(
  state: ClipboardSyncState,
  client: ClipboardSyncClient,
  io: ClipboardSyncIo,
  signal?: AbortSignal,
  isCurrent?: () => boolean,
): Promise<ClipboardSyncState> {
  assertRoundActive(signal, isCurrent);
  let result: HostClipboardResult;
  try {
    result = await client.request<HostClipboardResult>("getClipboard", {
      hash: state.lastHash,
    });
  } catch {
    assertRoundActive(signal, isCurrent);
    // 전송 오류·게이트 거부는 다음 폴링에서 다시 시도한다.
    return state;
  }
  // 요청 대기 중 라운드가 취소됐을 수 있다 — 기기 쓰기로 넘어가지 않는다.
  assertRoundActive(signal, isCurrent);
  if (result.unchanged || !result.hash) {
    return state;
  }
  if (typeof result.imageBase64 === "string") {
    assertRoundActive(signal, isCurrent);
    try {
      await io.writeDeviceClipboardImage(result.imageBase64);
      assertRoundActive(signal, isCurrent);
    } catch {
      // 쓰기 실패는 상태를 갱신하지 않는다 — 다음 폴링이 재시도한다.
      return state;
    }
    return {
      ...state,
      lastHash: result.hash,
      localText: "",
      localImageHash: result.hash,
    };
  }
  if (typeof result.text !== "string") {
    return state;
  }
  assertRoundActive(signal, isCurrent);
  try {
    await io.writeDeviceClipboard(result.text);
    assertRoundActive(signal, isCurrent);
  } catch {
    // 쓰기 실패는 상태를 갱신하지 않는다 — 다음 폴링이 같은 텍스트로 재시도한다.
    return state;
  }
  return { ...state, lastHash: result.hash, localText: result.text, localImageHash: "" };
}

/**
 * 기기 클립보드 변경 반영 한 번. 로컬 텍스트가 바뀌었고 그 해시가 호스트
 * 해시와 다를 때만 setClipboard으로 밀어 올린다. 취소된 라운드는 읽기·전송을
 * 시작하지 않는다.
 */
export async function pushDeviceClipboard(
  state: ClipboardSyncState,
  client: ClipboardSyncClient,
  io: ClipboardSyncIo,
  signal?: AbortSignal,
  isCurrent?: () => boolean,
): Promise<ClipboardSyncState> {
  if (state.gateRejections >= CLIPBOARD_GATE_LATCH_LIMIT) {
    // 게이트 꺼짐 래치 — 설정 토글로 재무장할 때까지 푸시를 시도하지 않는다.
    return state;
  }
  assertRoundActive(signal, isCurrent);
  let device: string;
  try {
    device = await io.readDeviceClipboard();
  } catch {
    return state;
  }
  // 읽기가 끝난 뒤에도 취소됐으면 밀어 올리지 않는다.
  assertRoundActive(signal, isCurrent);
  if (device.length === 0) {
    // 텍스트가 비어 있으면 이미지 클립보드를 본다(텍스트 우선). Android
    // 10+의 백그라운드 접근 거부는 빈 텍스트로 나타나는데, 빈 텍스트를
    // 밀면 호스트 클립보드가 지워지므로 빈 읽기는 이미지 조회 후 무시된다.
    return pushDeviceImage(state, client, io, signal, isCurrent);
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
    assertRoundActive(signal, isCurrent);
    await client.request("setClipboard", { text: device });
    assertRoundActive(signal, isCurrent);
  } catch (error) {
    return isClipboardGateError(error) ? gateRejected(state) : state;
  }
  return {
    ...state,
    lastHash: hash,
    localText: device,
    localImageHash: "",
    gateRejections: 0,
  };
}

/** 기기 이미지 클립보드 변경 반영 한 번. 빈 읽기(접근 거부 포함)는 무시한다. */
async function pushDeviceImage(
  state: ClipboardSyncState,
  client: ClipboardSyncClient,
  io: ClipboardSyncIo,
  signal?: AbortSignal,
  isCurrent?: () => boolean,
): Promise<ClipboardSyncState> {
  assertRoundActive(signal, isCurrent);
  let image: string | null;
  try {
    image = await io.readDeviceClipboardImage();
  } catch {
    return state;
  }
  assertRoundActive(signal, isCurrent);
  if (!image) return state;
  const hash = imageClipboardHash(image);
  // 우리가 방금 호스트에서 내려받아 쓴 이미지(에코)다 — 다시 올리지 않는다.
  if (hash === state.localImageHash || hash === state.lastHash) {
    return state;
  }
  try {
    assertRoundActive(signal, isCurrent);
    await client.request("setClipboard", { imageBase64: image });
    assertRoundActive(signal, isCurrent);
  } catch (error) {
    return isClipboardGateError(error) ? gateRejected(state) : state;
  }
  return { lastHash: hash, localText: "", localImageHash: "", gateRejections: 0 };
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
  // 제어 요청은 최대 15초까지 걸릴 수 있어 2.5초 틱이 겹친다 — 진행 중
  // 라운드가 있으면 다음 틱은 건너뛴다(상태 경합·중복 요청 방지).
  let inFlight = false;
  // 진행 중 라운드의 취소 지점 — setEnabled(false)·stop()이 abort하면 그
  // 라운드는 이미 시작한 패킷을 제외한 나머지 읽기·쓰기·전송을 하지 않는다.
  let round: AbortController | null = null;
  let disposeChanges: (() => void) | null = null;
  let changeGeneration = 0;
  let readGeneration = -1;
  let lastLocalReadAt = 0;
  let sourceClient: ClipboardSyncClient | null = null;
  let listenerLifetime = 0;

  const tick = async () => {
    if (!enabled || inFlight) return;
    inFlight = true;
    const controller = new AbortController();
    round = controller;
    try {
      const client = io.getClient();
      if (!client) return;
      if (sourceClient !== client) {
        sourceClient = client;
        readGeneration = -1;
      }
      const isCurrent = () => enabled && io.getClient() === client;
      state = await pollHostClipboard(state, client, io, controller.signal, isCurrent);
      if (!disposeChanges || readGeneration !== changeGeneration || Date.now() - lastLocalReadAt >= 30_000) {
        const readingGeneration = changeGeneration;
        state = await pushDeviceClipboard(state, client, io, controller.signal, isCurrent);
        assertRoundActive(controller.signal, isCurrent);
        readGeneration = readingGeneration;
        lastLocalReadAt = Date.now();
      }
    } catch {
      // AbortError(취소된 라운드)만 여기 온다 — 조용히 끝낸다.
    } finally {
      inFlight = false;
      if (round === controller) round = null;
    }
  };

  const loop: ClipboardSyncLoop = {
    setEnabled(next: boolean) {
      if (enabled === next) return;
      enabled = next;
      // 설정 변경은 게이트 꺼짐 래치의 재무장이다 — 사용자가 토글을 다시
      // 켜면 호스트 게이트가 바뀌었을 수 있으므로 푸시를 다시 시도한다.
      state = { ...state, gateRejections: 0 };
      const lifetime = ++listenerLifetime;
      if (enabled) {
        readGeneration = -1;
        try {
          disposeChanges = io.subscribeDeviceChanges?.(() => {
            if (enabled && listenerLifetime === lifetime) changeGeneration++;
          }) ?? null;
        } catch { disposeChanges = null; }
        timer = setInterval(() => void tick(), CLIPBOARD_SYNC_INTERVAL_MS);
      } else {
        disposeChanges?.();
        disposeChanges = null;
        sourceClient = null;
        round?.abort();
        if (timer !== null) {
          clearInterval(timer);
          timer = null;
        }
      }
    },
    isEnabled: () => enabled,
    stop() {
      loop.setEnabled(false);
    },
  };
  return loop;
}
