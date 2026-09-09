import { beforeEach, describe, expect, it, vi } from "vitest";

// clipboard-sync.ts는 expo-clipboard를 정적 import하므로 node 테스트 환경에서는
// 모듈 경계에서 가짜로 대체한다(실제 접근은 io 주입으로만 일어난다).
vi.mock("expo-clipboard", () => ({
  getStringAsync: vi.fn(async () => ""),
  setStringAsync: vi.fn(async () => undefined),
}));

import {
  INITIAL_CLIPBOARD_SYNC_STATE,
  CLIPBOARD_SYNC_INTERVAL_MS,
  clipboardHash,
  loadClipboardShare,
  parseClipboardShare,
  pollHostClipboard,
  pushDeviceClipboard,
  saveClipboardShare,
  startClipboardSync,
  type ClipboardSyncClient,
  type ClipboardSyncIo,
  type ClipboardSyncState,
} from "./clipboard-sync";

function fakeClient(handler: (command: string, args: unknown) => unknown): ClipboardSyncClient {
  const calls: Array<{ command: string; args: unknown }> = [];
  return {
    request: async <T,>(command: string, args?: unknown): Promise<T> => {
      calls.push({ command, args });
      return handler(command, args) as T;
    },
  };
}

interface IoHarness {
  io: ClipboardSyncIo;
  writes: string[];
  device: { text: string };
}

function fakeIo(initialDevice = ""): IoHarness {
  const device = { text: initialDevice };
  const writes: string[] = [];
  return {
    device,
    writes,
    io: {
      getClient: () => null,
      readDeviceClipboard: async () => device.text,
      writeDeviceClipboard: async (text: string) => {
        writes.push(text);
        device.text = text;
      },
    },
  };
}

function stateWith(lastHash: string, localText: string): ClipboardSyncState {
  return { lastHash, localText };
}

let memoryStore: Map<string, string>;

beforeEach(() => {
  memoryStore = new Map();
});

describe("clipboard share toggle persistence", () => {
  it("defaults to off for missing or unreadable values", async () => {
    expect(parseClipboardShare(null)).toBe(false);
    expect(parseClipboardShare("0")).toBe(false);
    expect(await loadClipboardShare({
      getItemAsync: async (key) => memoryStore.get(key) ?? null,
      setItemAsync: async (key, value) => void memoryStore.set(key, value),
    })).toBe(false);
  });

  it("persists the enabled choice and reads it back", async () => {
    await saveClipboardShare({
      getItemAsync: async (key) => memoryStore.get(key) ?? null,
      setItemAsync: async (key, value) => void memoryStore.set(key, value),
    }, true);
    expect(memoryStore.get("leftcar.clipboardShare")).toBe("1");
    expect(await loadClipboardShare({
      getItemAsync: async (key) => memoryStore.get(key) ?? null,
      setItemAsync: async (key, value) => void memoryStore.set(key, value),
    })).toBe(true);
  });
});

describe("pollHostClipboard", () => {
  it("keeps state untouched when the host answers unchanged", async () => {
    const harness = fakeIo();
    const client = fakeClient(() => ({ unchanged: true }));
    const state = stateWith(clipboardHash("호스트"), "호스트");
    const next = await pollHostClipboard(state, client, harness.io);
    expect(next).toBe(state);
    expect(harness.writes).toEqual([]);
  });

  it("writes new host text to the device and records its hash", async () => {
    const harness = fakeIo();
    const hash = clipboardHash("새 텍스트");
    const client = fakeClient((command, args) => {
      expect(command).toBe("getClipboard");
      expect(args).toEqual({ hash: INITIAL_CLIPBOARD_SYNC_STATE.lastHash });
      return { unchanged: false, text: "새 텍스트", hash };
    });
    const next = await pollHostClipboard(INITIAL_CLIPBOARD_SYNC_STATE, client, harness.io);
    expect(harness.writes).toEqual(["새 텍스트"]);
    expect(next.lastHash).toBe(hash);
    expect(next.localText).toBe("새 텍스트");
  });

  it("survives transport errors and failed device writes without state churn", async () => {
    const harness = fakeIo();
    const failing: ClipboardSyncClient = {
      request: vi.fn(async () => {
        throw new Error("control connection closed");
      }),
    };
    const state = stateWith(clipboardHash("x"), "x");
    expect(await pollHostClipboard(state, failing, harness.io)).toBe(state);

    const writeFailure: ClipboardSyncClient = fakeClient(() => ({
      unchanged: false,
      text: "새 텍스트",
      hash: clipboardHash("새 텍스트"),
    }));
    harness.io.writeDeviceClipboard = async () => {
      throw new Error("write failed");
    };
    expect(await pollHostClipboard(state, writeFailure, harness.io)).toBe(state);
  });
});

describe("pushDeviceClipboard", () => {
  it("uploads local changes that differ from the host hash", async () => {
    const harness = fakeIo();
    harness.device.text = "기기에서 복사";
    const sent: Array<{ command: string; args: unknown }> = [];
    const client = fakeClient((command, args) => {
      sent.push({ command, args });
      return {};
    });
    const state = stateWith(clipboardHash("호스트"), "호스트");
    const next = await pushDeviceClipboard(state, client, harness.io);
    expect(sent).toEqual([
      { command: "setClipboard", args: { text: "기기에서 복사" } },
    ]);
    expect(next.localText).toBe("기기에서 복사");
    expect(next.lastHash).toBe(clipboardHash("기기에서 복사"));
  });

  it("ignores the echo of text the host already sent", async () => {
    const harness = fakeIo();
    harness.device.text = "호스트 텍스트";
    const client = fakeClient(() => {
      throw new Error("setClipboard must not be called for echoes");
    });
    const state = stateWith(clipboardHash("호스트 텍스트"), "호스트 텍스트");
    const next = await pushDeviceClipboard(state, client, harness.io);
    expect(next).toBe(state);
  });

  it("does not re-upload when the device text is unchanged", async () => {
    const harness = fakeIo();
    harness.device.text = "같은 텍스트";
    const client = fakeClient(() => {
      throw new Error("setClipboard must not run without a local change");
    });
    const state = stateWith(clipboardHash("다른 곳"), "같은 텍스트");
    const next = await pushDeviceClipboard(state, client, harness.io);
    // 해시가 호스트 해시와 같지 않아도 로컬 텍스트 그대로면 밀어 올리지 않는다.
    expect(next).toBe(state);
  });
});

describe("startClipboardSync loop", () => {
  it("stops polling when the toggle turns off", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      const requests: string[] = [];
      harness.io.getClient = () =>
        fakeClient((command) => {
          requests.push(command);
          return { unchanged: true };
        });
      const loop = startClipboardSync(harness.io);
      expect(loop.isEnabled()).toBe(false);

      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      const whileOn = requests.length;
      expect(whileOn).toBeGreaterThan(0);

      loop.setEnabled(false);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS * 4);
      expect(requests.length).toBe(whileOn);

      loop.stop();
      expect(loop.isEnabled()).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips rounds without a control session but resumes when one appears", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      let client: ClipboardSyncClient | null = null;
      harness.io.getClient = () => client;
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);

      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS * 2);
      expect(harness.writes).toEqual([]);

      let polls = 0;
      client = fakeClient((command) => {
        if (command === "getClipboard") polls += 1;
        return {
          unchanged: false,
          text: "호스트에서 온 텍스트",
          hash: clipboardHash("호스트에서 온 텍스트"),
        };
      });
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(polls).toBe(1);
      expect(harness.writes).toEqual(["호스트에서 온 텍스트"]);
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("runs a full round trip: host push then device echo stays silent", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      const hostText = "왕복 텍스트";
      harness.io.getClient = () =>
        fakeClient((command) => {
          if (command === "getClipboard") {
            return { unchanged: false, text: hostText, hash: clipboardHash(hostText) };
          }
          // setClipboard 에코는 호스트가 무시한다(해시 동일) — 성공으로 답한다.
          return {};
        });
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(harness.writes).toEqual([hostText]);

      // 두 번째 라운드: 기기 텍스트는 호스트 텍스트와 같다(에코) → 밀어 올리지 않는다.
      let setCalls = 0;
      harness.io.getClient = () =>
        fakeClient((command) => {
          if (command === "setClipboard") setCalls += 1;
          if (command === "getClipboard") return { unchanged: true };
          return {};
        });
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(setCalls).toBe(0);
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
