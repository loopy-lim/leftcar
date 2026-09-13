import { beforeEach, describe, expect, it, vi } from "vitest";

// clipboard-sync.ts는 expo-clipboard를 정적 import하므로 node 테스트 환경에서는
// 모듈 경계에서 가짜로 대체한다(실제 접근은 io 주입으로만 일어난다).
vi.mock("expo-clipboard", () => ({
  getStringAsync: vi.fn(async () => ""),
  setStringAsync: vi.fn(async () => undefined),
}));

import {
  INITIAL_CLIPBOARD_SYNC_STATE,
  CLIPBOARD_GATE_LATCH_LIMIT,
  CLIPBOARD_SYNC_INTERVAL_MS,
  clipboardHash,
  loadClipboardShare,
  parseClipboardShare,
  pollHostClipboard,
  imageClipboardHash,
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
  imageWrites: string[];
  device: { text: string; image: string | null };
}

function fakeIo(initialDevice = ""): IoHarness {
  const device = { text: initialDevice, image: null as string | null };
  const writes: string[] = [];
  const imageWrites: string[] = [];
  return {
    device,
    writes,
    imageWrites,
    io: {
      getClient: () => null,
      readDeviceClipboard: async () => device.text,
      writeDeviceClipboard: async (text: string) => {
        writes.push(text);
        device.text = text;
      },
      readDeviceClipboardImage: async () => device.image,
      writeDeviceClipboardImage: async (base64: string) => {
        imageWrites.push(base64);
        device.image = base64;
      },
    },
  };
}

function stateWith(lastHash: string, localText: string): ClipboardSyncState {
  return { lastHash, localText, localImageHash: "", gateRejections: 0 };
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

  it("surfaces failed and malformed reads without inventing an off value", async () => {
    await expect(loadClipboardShare({
      getItemAsync: async () => {
        throw new Error("secure read failed");
      },
      setItemAsync: async () => undefined,
    })).rejects.toThrow("secure read failed");

    await expect(loadClipboardShare({
      getItemAsync: async () => "unexpected",
      setItemAsync: async () => undefined,
    })).rejects.toThrow("Invalid clipboard preference");
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

describe("pollHostClipboard image", () => {
  it("writes a host image to the device clipboard and records its hash", async () => {
    const harness = fakeIo();
    const image = "aG9zdC1pbWFnZQ==";
    const hash = imageClipboardHash(image);
    const client = fakeClient(() => ({ unchanged: false, imageBase64: image, hash }));
    const next = await pollHostClipboard(
      stateWith(clipboardHash("old"), "old"),
      client,
      harness.io,
    );
    expect(harness.imageWrites).toEqual([image]);
    expect(next.lastHash).toBe(hash);
    expect(next.localImageHash).toBe(hash);
    expect(next.localText).toBe("");
  });

  it("leaves state alone when the host reports an image unchanged", async () => {
    const harness = fakeIo();
    const image = "aG9zdC1pbWFnZQ==";
    const client = fakeClient(() => ({ unchanged: true }));
    const state = stateWith(imageClipboardHash(image), "");
    const next = await pollHostClipboard(state, client, harness.io);
    expect(next).toBe(state);
    expect(harness.imageWrites).toEqual([]);
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

  it("pushes a device image when the text clipboard is empty", async () => {
    const harness = fakeIo();
    harness.device.image = "aW1hZ2UtcG5n";
    const sent: Array<{ command: string; args: unknown }> = [];
    const client = fakeClient((command, args) => {
      sent.push({ command, args });
      return {};
    });
    const state = stateWith(clipboardHash("호스트"), "호스트");
    const next = await pushDeviceClipboard(state, client, harness.io);
    expect(sent).toHaveLength(1);
    expect(sent[0].command).toBe("setClipboard");
    expect((sent[0].args as { imageBase64: string }).imageBase64).toBe("aW1hZ2UtcG5n");
    expect(next.lastHash).toBe(`i:${clipboardHash("aW1hZ2UtcG5n")}`);
  });

  it("does not re-push the image the host just sent down (echo)", async () => {
    const harness = fakeIo();
    const image = "aG9zdC1pbWFnZQ==";
    const client = fakeClient(() => {
      throw new Error("setClipboard must not run for image echoes");
    });
    // 폴링이 호스트 이미지를 기기에 쓴 직후의 상태다.
    harness.device.text = "";
    harness.device.image = image;
    const state: ClipboardSyncState = {
      lastHash: imageClipboardHash(image),
      localText: "",
      localImageHash: imageClipboardHash(image),
      gateRejections: 0,
    };
    const next = await pushDeviceClipboard(state, client, harness.io);
    expect(next).toBe(state);
  });

  it("never pushes an empty device read (background access denial)", async () => {
    // Android 10+ 백그라운드 클립보드 접근 거부는 expo-clipboard에서 빈
    // 문자열로 나타난다 — 이를 밀면 호스트 클립보드가 지워진다.
    const harness = fakeIo();
    harness.device.text = "";
    const client = fakeClient(() => {
      throw new Error("setClipboard must not run for empty device reads");
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

  it("latches pushes after repeated host gate rejections", async () => {
    const harness = fakeIo();
    harness.device.text = "기기에서 복사";
    let setCalls = 0;
    const client = fakeClient(() => {
      setCalls += 1;
      throw new Error("clipboard share disabled");
    });
    let state = stateWith(clipboardHash("호스트"), "호스트");
    // 임계치까지는 폴링마다 다시 시도한다(일시적 게이트 토글과의 경합을 줄
    // 여유) — 거부마다 카운터가 올라가고, 푸시가 실패했으므로 localText는
    // 그대로여서 다음 라운드도 같은 텍스트를 밀려고 한다.
    for (let round = 0; round < CLIPBOARD_GATE_LATCH_LIMIT; round += 1) {
      state = await pushDeviceClipboard(state, client, harness.io);
    }
    expect(setCalls).toBe(CLIPBOARD_GATE_LATCH_LIMIT);
    expect(state.gateRejections).toBe(CLIPBOARD_GATE_LATCH_LIMIT);
    // 래치 후에는 시도조차 하지 않는다(2.5초마다의 영원한 재시도를 끊는다).
    const next = await pushDeviceClipboard(state, client, harness.io);
    expect(setCalls).toBe(CLIPBOARD_GATE_LATCH_LIMIT);
    expect(next).toBe(state);
  });

  it("keeps retrying pushes on errors that are not gate rejections", async () => {
    const harness = fakeIo();
    harness.device.text = "기기에서 복사";
    let setCalls = 0;
    const client = fakeClient(() => {
      setCalls += 1;
      throw new Error("control request timeout");
    });
    let state = stateWith(clipboardHash("호스트"), "호스트");
    for (let round = 0; round < CLIPBOARD_GATE_LATCH_LIMIT * 2; round += 1) {
      state = await pushDeviceClipboard(state, client, harness.io);
    }
    // 게이트 오류가 아닌 전송 실패는 래치하지 않는다.
    expect(setCalls).toBe(CLIPBOARD_GATE_LATCH_LIMIT * 2);
    expect(state.gateRejections).toBe(0);
  });

  it("re-arms the gate counter after a successful push", async () => {
    const harness = fakeIo();
    harness.device.text = "기기에서 복사";
    const rejecting: ClipboardSyncClient = fakeClient(() => {
      throw new Error("clipboard share disabled");
    });
    let state = stateWith(clipboardHash("호스트"), "호스트");
    state = await pushDeviceClipboard(state, rejecting, harness.io);
    expect(state.gateRejections).toBe(1);

    const success = fakeClient(() => ({}));
    const next = await pushDeviceClipboard(state, success, harness.io);
    expect(next.gateRejections).toBe(0);
  });
});

describe("startClipboardSync loop", () => {
  it("stops polling when the toggle turns off", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      const requests: string[] = [];
      const client = fakeClient((command) => {
          requests.push(command);
          return { unchanged: true };
        });
      harness.io.getClient = () => client;
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
      const firstClient = fakeClient((command) => {
          if (command === "getClipboard") {
            return { unchanged: false, text: hostText, hash: clipboardHash(hostText) };
          }
          // setClipboard 에코는 호스트가 무시한다(해시 동일) — 성공으로 답한다.
          return {};
        });
      harness.io.getClient = () => firstClient;
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(harness.writes).toEqual([hostText]);

      // 두 번째 라운드: 기기 텍스트는 호스트 텍스트와 같다(에코) → 밀어 올리지 않는다.
      let setCalls = 0;
      const secondClient = fakeClient((command) => {
          if (command === "setClipboard") setCalls += 1;
          if (command === "getClipboard") return { unchanged: true };
          return {};
        });
      harness.io.getClient = () => secondClient;
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(setCalls).toBe(0);
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops pushing after gate rejections and re-arms on a settings change", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      harness.device.text = "기기에서 복사";
      let setCalls = 0;
      const client = fakeClient((command) => {
          if (command === "setClipboard") {
            setCalls += 1;
            throw new Error("clipboard share disabled");
          }
          return { unchanged: true };
        });
      harness.io.getClient = () => client;
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      // 임계치 이후 틱에서는 setClipboard 시도가 없다.
      await vi.advanceTimersByTimeAsync(
        CLIPBOARD_SYNC_INTERVAL_MS * (CLIPBOARD_GATE_LATCH_LIMIT + 2),
      );
      expect(setCalls).toBe(CLIPBOARD_GATE_LATCH_LIMIT);

      // 설정 토글(끄고 다시 켜기)은 래치를 재무장한다.
      loop.setEnabled(false);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(setCalls).toBe(CLIPBOARD_GATE_LATCH_LIMIT + 1);
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips a tick while the previous round is still in flight", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      let polls = 0;
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const client = fakeClient(async (command) => {
          if (command === "getClipboard") {
            polls += 1;
            // 첫 라운드가 15초 제어 타임아웃에 붙잡혀 있는 상태를 흉내 낸다.
            await gate;
          }
          return { unchanged: true };
        });
      harness.io.getClient = () => client;
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(polls).toBe(1);
      // 진행 중 라운드가 끝나기 전까지 이후 틱은 건너뛴다.
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS * 3);
      expect(polls).toBe(1);
      release();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(polls).toBe(2);
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("abandons the in-flight round when the toggle turns off — no write, read, or send", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      let releaseGet!: () => void;
      const gate = new Promise<void>((resolve) => {
        releaseGet = resolve;
      });
      const sent: string[] = [];
      const client = fakeClient(async (command) => {
          sent.push(command);
          if (command === "getClipboard") {
            await gate;
            return {
              unchanged: false,
              text: "늦게 도착한 텍스트",
              hash: clipboardHash("늦게 도착한 텍스트"),
            };
          }
          return {};
        });
      harness.io.getClient = () => client;
      const deviceReads: number[] = [];
      const realRead = harness.io.readDeviceClipboard;
      harness.io.readDeviceClipboard = async () => {
        deviceReads.push(deviceReads.length + 1);
        return realRead();
      };

      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(sent).toEqual(["getClipboard"]);

      // getClipboard 응답을 기다리는 동안 토글을 끈다 → 라운드는 취소된다.
      loop.setEnabled(false);
      releaseGet();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);

      expect(sent).toEqual(["getClipboard"]); // setClipboard 없음
      expect(harness.writes).toEqual([]); // writeDeviceClipboard 없음
      expect(deviceReads).toEqual([]); // 기기 읽기도 시작하지 않는다
    } finally {
      vi.useRealTimers();
    }
  });

  it("stop() cancels the in-flight round before the image write", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      let releaseGet!: () => void;
      const gate = new Promise<void>((resolve) => {
        releaseGet = resolve;
      });
      const sent: string[] = [];
      const client = fakeClient(async (command) => {
          sent.push(command);
          if (command === "getClipboard") {
            await gate;
            const image = "aG9zdC1pbWFnZQ==";
            return { unchanged: false, imageBase64: image, hash: imageClipboardHash(image) };
          }
          return {};
        });
      harness.io.getClient = () => client;
      let imageReads = 0;
      const realReadImage = harness.io.readDeviceClipboardImage;
      harness.io.readDeviceClipboardImage = async () => {
        imageReads += 1;
        return realReadImage();
      };

      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(sent).toEqual(["getClipboard"]);

      loop.stop();
      releaseGet();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS * 2);

      expect(sent).toEqual(["getClipboard"]);
      expect(harness.imageWrites).toEqual([]); // writeDeviceClipboardImage 없음
      expect(imageReads).toBe(0); // pushDeviceClipboard 자체가 시작하지 않는다
    } finally {
      vi.useRealTimers();
    }
  });

  it("abandons an old host round when the control client is replaced during its poll", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo("new local value");
      let releaseOld!: () => void;
      const oldGate = new Promise<void>((resolve) => {
        releaseOld = resolve;
      });
      const oldClient = fakeClient(async (command) => {
        if (command === "getClipboard") {
          await oldGate;
          throw new Error("old socket closed");
        }
        throw new Error(`stale side effect: ${command}`);
      });
      const newClient = fakeClient(() => ({ unchanged: true }));
      let currentClient: ClipboardSyncClient | null = oldClient;
      harness.io.getClient = () => currentClient;
      const readDevice = vi.spyOn(harness.io, "readDeviceClipboard");

      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      currentClient = newClient;
      releaseOld();
      await vi.advanceTimersByTimeAsync(0);

      expect(readDevice).not.toHaveBeenCalled();
      loop.stop();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("round cancellation signal", () => {
  it("pollHostClipboard skips the device write when the signal aborted during the request", async () => {
    const harness = fakeIo();
    const client = fakeClient(() => ({
      unchanged: false,
      text: "늦은 텍스트",
      hash: clipboardHash("늦은 텍스트"),
    }));
    const controller = new AbortController();
    const pending = pollHostClipboard(
      INITIAL_CLIPBOARD_SYNC_STATE,
      client,
      harness.io,
      controller.signal,
    );
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(harness.writes).toEqual([]);
    expect(harness.imageWrites).toEqual([]);
  });

  it("pushDeviceClipboard never sends when aborted after the device read", async () => {
    const harness = fakeIo();
    harness.device.text = "기기에서 복사";
    const sent: Array<{ command: string; args: unknown }> = [];
    const client = fakeClient((command, args) => {
      sent.push({ command, args });
      return {};
    });
    const controller = new AbortController();
    const pending = pushDeviceClipboard(
      stateWith(clipboardHash("호스트"), "호스트"),
      client,
      harness.io,
      controller.signal,
    );
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(sent).toEqual([]);
  });
});

describe("native clipboard change signaling", () => {
  it("skips unchanged image reads, recovers lost events, and disposes retired listeners", async () => {
    vi.useFakeTimers();
    try {
      const harness = fakeIo();
      const client = fakeClient(() => ({ unchanged: true }));
      harness.io.getClient = () => client;
      let notify = () => {};
      const dispose = vi.fn();
      harness.io.subscribeDeviceChanges = (listener) => { notify = listener; return dispose; };
      const read = vi.spyOn(harness.io, "readDeviceClipboardImage");
      const loop = startClipboardSync(harness.io);
      loop.setEnabled(true);
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS * 3);
      expect(read).toHaveBeenCalledTimes(1);
      notify();
      await vi.advanceTimersByTimeAsync(CLIPBOARD_SYNC_INTERVAL_MS);
      expect(read).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(30_000);
      expect(read.mock.calls.length).toBeGreaterThan(2);
      loop.stop();
      expect(dispose).toHaveBeenCalledTimes(1);
      const count = read.mock.calls.length;
      notify();
      await vi.advanceTimersByTimeAsync(30_000);
      expect(read).toHaveBeenCalledTimes(count);
    } finally { vi.useRealTimers(); }
  });
});
