import { beforeEach, describe, expect, it, vi } from "vitest";
import { KmSession, type KmNative, type KmNativeStatus, type KmPeer } from "./km-session";

// Minimal hook dispatcher fixture; the production model, reducer, action callbacks,
// status subscription and KmSession are real. This is not a React renderer/device test.
const fixture = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, mounting: true,
  effects: [] as (() => unknown)[],
  native: null as KmNative | null, session: null as KmSession | null,
  listener: null as ((status: KmNativeStatus) => void) | null,
}));
vi.mock("react", () => ({
  useCallback: <T>(callback: T) => callback,
  useReducer: <S, A>(reducer: (state: S, action: A) => S, initial: S) => {
    const index = fixture.cursor++;
    if (fixture.mounting) fixture.slots[index] = initial;
    return [fixture.slots[index] as S, (action: A) => { fixture.slots[index] = reducer(fixture.slots[index] as S, action); }];
  },
  useRef: <T>(initial: T) => {
    const index = fixture.cursor++;
    if (fixture.mounting) fixture.slots[index] = { current: initial };
    return fixture.slots[index] as { current: T };
  },
  useEffect: (effect: () => unknown) => { if (fixture.mounting) fixture.effects.push(effect); },
}));
vi.mock("expo-router", () => ({ useFocusEffect: (effect: () => unknown) => { if (fixture.mounting) fixture.effects.push(effect); } }));
vi.mock("react-native", () => ({ AppState: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("./session", () => ({ controlTarget: () => null }));
vi.mock("./km.native", () => ({
  get kmNative() { return fixture.native; }, get kmSession() { return fixture.session; },
  subscribeKmStatus: (listener: (status: KmNativeStatus) => void) => { fixture.listener = listener; return () => { fixture.listener = null; }; },
}));
import { useKmModel } from "./use-km-model";

const ready: KmNativeStatus = { state: "preparing", shizukuRunning: true, shizukuGranted: true, accessibilityEnabled: true,
  displayWidth: 2560, displayHeight: 1600,
  capabilities: { cursorInjection: true, mouseObservation: true, mouseCapture: true, keyboardCapture: true } };
function deferred<T>() {
  let resolve!: (value: T) => void; let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((okay, fail) => { resolve = okay; reject = fail; });
  return { promise, resolve, reject };
}
function readModel() { fixture.cursor = 0; return useKmModel(); }
async function mountModel(overrides: Partial<KmNative> = {}) {
  let status: KmNativeStatus = { ...ready, state: "off", reason: null };
  let current = true;
  const effects: string[] = [];
  const emit = (next: KmNativeStatus) => { status = next; fixture.listener?.(next); };
  fixture.native = {
    getStatus: async () => status, prepare: async () => ready,
    start: async () => { const armed = { ...ready, state: "armed" as const }; emit(armed); return armed; },
    stop: async () => { effects.push("nativeStop"); emit({ ...ready, state: "off", reason: null }); }, ...overrides,
  };
  const peer: KmPeer = { host: "192.168.1.8", isCurrent: () => current, close: () => { effects.push("close"); },
    request: async <T>(command: string) => {
      effects.push(command);
      return { sessionId: "fixture", sessionKey: "fixture", epoch: 1, port: 8888, status: {} } as T;
    } };
  fixture.session = new KmSession({ native: fixture.native, openPeer: async () => peer });
  fixture.session.setOptions({ arrangementSide: "right", macToAndroid: true, androidToMac: true });
  const model = readModel(); fixture.mounting = false;
  for (const effect of fixture.effects) effect();
  await Promise.resolve(); await Promise.resolve();
  return { model, emit, effects, makeStale: () => { current = false; } };
}
beforeEach(() => {
  fixture.slots = []; fixture.cursor = 0; fixture.effects = []; fixture.mounting = true;
  fixture.native = null; fixture.session = null; fixture.listener = null;
});
describe("input-sharing model startup retirement", () => {
  it.each(["cancel", "stale"] as const)("does not present ordinary status reasons before or after a %s rejection retires", async retirement => {
    const entered = deferred<void>(); const preparation = deferred<KmNativeStatus>();
    const { model, emit, effects, makeStale } = await mountModel({ prepare: async () => { entered.resolve(); return preparation.promise; } });
    model.start(); await entered.promise;
    if (retirement === "cancel") model.cancelStart(); else makeStale();
    const rejected = { ...ready, state: "off" as const, reason: "Shizuku 입력 서비스 연결 시간이 초과되었습니다" };
    emit(rejected); preparation.reject(new Error(rejected.reason));
    await vi.waitFor(() => expect(readModel().busy).toBe(false));
    expect(readModel()).toMatchObject({ error: null, failure: null, status: { state: "off", reason: null } });
    expect(effects).toEqual(["nativeStop", "close"]);
    emit(rejected); // A delayed native reason from the same retired startup.
    expect(readModel()).toMatchObject({ error: null, failure: null, status: { state: "off", reason: null } });
    expect(fixture.session?.getPeerHost()).toBeNull();
  });
  it("keeps a genuine active automatic-stop reason through reasonless cleanup", async () => {
    const { model, emit, effects } = await mountModel();
    model.start(); await vi.waitFor(() => expect(readModel().busy).toBe(false));
    expect(fixture.session?.getPeerHost()).toBe("192.168.1.8");
    emit({ ...ready, state: "off", reason: "입력 공유 응답 또는 권한이 만료되었습니다" });
    await vi.waitFor(() => expect(effects.at(-1)).toBe("close"));
    expect(readModel()).toMatchObject({ error: "permissionsLost", status: { state: "off" } });
    expect(fixture.session?.getPeerHost()).toBeNull();
  });
  it("keeps cleanupFailed visible when cancellation cleanup rejects", async () => {
    const entered = deferred<void>(); const preparation = deferred<KmNativeStatus>();
    const { model, emit } = await mountModel({ prepare: async () => { entered.resolve(); return preparation.promise; },
      stop: async () => { emit({ ...ready, state: "cleanupFailed", reason: "cleanup fixture" }); throw new Error("cleanup fixture"); } });
    model.start(); await entered.promise; model.cancelStart();
    preparation.reject(new Error("Shizuku 입력 서비스 연결 시간이 초과되었습니다"));
    await vi.waitFor(() => expect(readModel().busy).toBe(false));
    expect(readModel()).toMatchObject({ error: "stopFailed", status: { state: "cleanupFailed" } });
  });
  it("keeps a late cleanupFailed status visible after silent cancellation", async () => {
    const entered = deferred<void>(); const preparation = deferred<KmNativeStatus>();
    const { model, emit } = await mountModel({ prepare: async () => { entered.resolve(); return preparation.promise; } });
    model.start(); await entered.promise; model.cancelStart(); preparation.reject(new Error("native fixture"));
    await vi.waitFor(() => expect(readModel().busy).toBe(false));
    emit({ ...ready, state: "cleanupFailed", reason: "cleanup fixture" });
    await Promise.resolve();
    expect(readModel()).toMatchObject({ error: "stopFailed", status: { state: "cleanupFailed" } });
  });
});
