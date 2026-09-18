import { describe, expect, it, vi } from "vitest";
import { StreamSessionStore } from "./stream-session-store";
import { StreamRecoveryController } from "./stream-recovery";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { StreamTerminationEvent } from "./stream-termination-policy";
import { ReservedStream } from "./reserved-stream";
import { DecoderReservations } from "./decoder-budget";

function active(session = 6): ActiveStream {
  const target = { width: 3840, height: 2160, fps: 60 };
  return { session, port: 5007, sourceIndex: 0, sourceName: "Display 0", ...target,
    sourceTarget: target, activeTarget: target, fallbackTarget: null, qualityState: "native",
    captureBackend: "screenCaptureKit", contentMode: "interactive", encoderExperiment: "splitVertical",
    viewerIps: ["100.77.109.50"], mediaTransport: "udp", mediaKey: "fixture", startedAt: 1 };
}

function setup(restore: (stream: ActiveStream) => Promise<RestoredStream> = async s => ({ ...s, session: 7 })) {
  const store = new StreamSessionStore();
  let listener: ((event: StreamTerminationEvent) => void) | undefined;
  const stopped: number[] = [];
  const controller = new StreamRecoveryController(store, {
    subscribe: next => { listener = next; return { remove: () => { listener = undefined; } }; },
    restore, stop: async session => { stopped.push(session); },
  });
  store.update(() => [active()]);
  return { store, controller, stopped,
    emit: (event: StreamTerminationEvent) => listener?.(event),
    listening: () => listener !== undefined };
}

async function settle() { for (let i = 0; i < 12; i++) await Promise.resolve(); }

describe("recovery owned by native windows", () => {
  it("restores split video after the catalog unsubscribes and detaches", async () => {
    const h = setup();
    const unsubscribe = h.store.subscribe(() => {});
    unsubscribe(); h.store.detachCatalog();
    h.emit({ port: 5007, reason: 5 });
    await settle();
    expect(h.store.getSnapshot().map(s => s.session)).toEqual([7]);
    expect(h.store.getSnapshot()[0]?.port).toBe(5007);
    expect(h.listening()).toBe(true);
    h.store.update(() => []);
    expect(h.listening()).toBe(false);
  });

  it("removes a user-closed window immediately so later recovery cannot reopen it", async () => {
    const h = setup(); h.store.detachCatalog();
    h.emit({ port: 5007, reason: 0 });
    expect(h.store.getSnapshot()).toEqual([]);
    h.emit({ port: 5007, reason: 5 });
    await settle();
    expect(h.stopped).toEqual([6]);
    expect(h.listening()).toBe(false);
  });

  it("stops a late replacement when the window was closed during recovery", async () => {
    let finish!: (s: RestoredStream) => void;
    const h = setup(() => new Promise(resolve => { finish = resolve; }));
    h.emit({ port: 5007, reason: 5 });
    h.emit({ port: 5007, reason: 0 });
    finish({ ...active(), session: 7 });
    await settle();
    expect(h.store.getSnapshot()).toEqual([]);
    expect(h.stopped).toEqual([6, 7]);
    expect(h.store.recoveryInFlight.size).toBe(0);
  });

  it("coalesces native failure notifications while one restore is pending", async () => {
    let finish!: (s: RestoredStream) => void;
    let nextSession = 7;
    const h = setup(() => { const session = nextSession++; return new Promise(resolve => { finish = s => resolve({ ...s, session }); }); });
    h.emit({ port: 5007, reason: 1 });
    h.emit({ port: 5007, reason: 4 });
    h.emit({ port: 5007, reason: 5 });
    finish(active()); await settle();
    expect(h.store.getSnapshot()[0]?.session).toBe(7);
    expect(nextSession).toBe(8);
    h.store.update(() => []);
  });

  it("shares the recovery claim with catalog actions", async () => {
    const h = setup(); h.store.recoveryInFlight.add(6);
    h.emit({ port: 5007, reason: 5 }); await settle();
    expect(h.store.getSnapshot()[0]?.session).toBe(6);
    h.store.recoveryInFlight.delete(6);
    h.emit({ port: 5007, reason: 5 }); await settle();
    expect(h.store.getSnapshot()[0]?.session).toBe(7);
    h.store.update(() => []);
  });

  it("keeps a failed window retryable without requiring the catalog", async () => {
    vi.useFakeTimers();
    try {
      let fail = true;
      const h = setup(async s => { if (fail) throw new Error("network unavailable"); return { ...s, session: 7 }; });
      h.emit({ port: 5007, reason: 5 }); await settle();
      expect(h.store.getRecoveryError()).toBe("network unavailable");
      expect(h.store.getSnapshot()[0]?.session).toBe(6);
      expect(h.store.recoveryInFlight.size).toBe(0);
      fail = false;
      h.emit({ port: 5007, reason: 5 }); await settle();
      // 실패 재시도 타이머를 기다리지 않는다 — 새 이벤트가 타이머를 대체해
      // 즉시 복원한다(창 포그라운드에선 RN 타이머가 멈추므로).
      expect(h.store.getSnapshot()[0]?.session).toBe(7);
      expect(h.store.getRecoveryError()).toBeNull();
      h.store.update(() => []);
    } finally { vi.useRealTimers(); }
  });

  it("retries a failed restore automatically until the host returns", async () => {
    vi.useFakeTimers();
    try {
      let attempts = 0;
      const h = setup(async s => { attempts += 1; if (attempts < 3) throw new Error("host down"); return { ...s, session: 7 }; });
      // 호스트 다운 중 첫 시도는 반드시 실패하고, 실패 이벤트는 다시 오지
      // 않는다(단발 계측). 백오프(2s → 4s) 자동 재시도로 복구한다.
      h.emit({ port: 5007, reason: 1 }); await settle();
      expect(attempts).toBe(1);
      expect(h.store.getRecoveryError()).toBe("host down");
      await vi.advanceTimersByTimeAsync(2_000); await settle();
      expect(attempts).toBe(2);
      expect(h.store.getRecoveryError()).toBe("host down");
      await vi.advanceTimersByTimeAsync(4_000); await settle();
      expect(attempts).toBe(3);
      expect(h.store.getSnapshot()[0]?.session).toBe(7);
      expect(h.store.getRecoveryError()).toBeNull();
      h.store.update(() => []);
    } finally { vi.useRealTimers(); }
  });

  it("stops automatic retries once the window closes", async () => {
    vi.useFakeTimers();
    try {
      let attempts = 0;
      const h = setup(async s => { attempts += 1; throw new Error("host down"); });
      h.emit({ port: 5007, reason: 4 }); await settle();
      expect(attempts).toBe(1);
      h.store.update(() => []);
      await vi.advanceTimersByTimeAsync(60_000); await settle();
      expect(attempts).toBe(1);
    } finally { vi.useRealTimers(); }
  });

  it("backs off repeated native terminations instead of stampeding recreates", async () => {
    vi.useFakeTimers();
    try {
      let nextSession = 7;
      const restores: number[] = [];
      const h = setup(async s => { const session = nextSession++; restores.push(s.session); return { ...s, session }; });
      h.emit({ port: 5007, reason: 4 });
      await settle();
      expect(restores).toEqual([6]);
      expect(h.store.getSnapshot()[0]?.session).toBe(7);
      // Same collapse, seconds later: the recreate waits out the 2s backoff.
      // (성공 뒤 백오프는 재생성 폭주 방지 — 실패 재시도와 달리 새 이벤트로
      // 대체되지 않는다.)
      h.emit({ port: 5007, reason: 4 });
      await settle();
      expect(restores).toEqual([6]);
      expect(h.store.getSnapshot()[0]?.session).toBe(7);
      await vi.advanceTimersByTimeAsync(2_000);
      expect(restores).toEqual([6, 7]);
      expect(h.store.getSnapshot()[0]?.session).toBe(8);
      h.store.update(() => []);
    } finally { vi.useRealTimers(); }
  });

  it("supersedes a failed-restore retry timer when a fresh event arrives", async () => {
    vi.useFakeTimers();
    try {
      let fail = true;
      const h = setup(async s => { if (fail) throw new Error("host down"); return { ...s, session: 7 }; });
      h.emit({ port: 5007, reason: 1 }); await settle();
      expect(h.store.getSnapshot()[0]?.session).toBe(6);
      expect(h.store.getRecoveryError()).toBe("host down");
      // 호스트 복귀를 알리는 재발행 이벤트 — 2s 실패-재시도 타이머를 기다리지
      // 않고 즉시 복원한다.
      fail = false;
      h.emit({ port: 5007, reason: 1 }); await settle();
      expect(h.store.getSnapshot()[0]?.session).toBe(7);
      expect(h.store.getRecoveryError()).toBeNull();
      h.store.update(() => []);
    } finally { vi.useRealTimers(); }
  });

  it("resets the backoff after a quiet minute", async () => {
    vi.useFakeTimers();
    try {
      const restores: number[] = [];
      const h = setup(async s => { restores.push(s.session); return { ...s, session: 7 }; });
      h.emit({ port: 5007, reason: 5 }); await settle();
      expect(restores).toEqual([6]);
      await vi.advanceTimersByTimeAsync(60_000);
      // A stream that lived a full minute turns the next failure into a
      // fresh incident: restore is immediate again.
      h.emit({ port: 5007, reason: 5 }); await settle();
      expect(restores).toEqual([6, 7]);
      h.store.update(() => []);
    } finally { vi.useRealTimers(); }
  });

  it("drops a pending backoff restore when the window closes during the wait", async () => {
    vi.useFakeTimers();
    try {
      const restores: number[] = [];
      const h = setup(async s => { restores.push(s.session); return { ...s, session: 7 }; });
      h.emit({ port: 5007, reason: 5 }); await settle();
      expect(restores).toEqual([6]);
      h.emit({ port: 5007, reason: 5 });
      h.store.update(() => []);
      await vi.advanceTimersByTimeAsync(4_000);
      expect(restores).toEqual([6]);
    } finally { vi.useRealTimers(); }
  });

  it("ignores invalid ports and terminal reasons instead of closing another window", async () => {
    const h = setup();
    h.emit({ port: 5999, reason: 0 });
    h.emit({ port: 5007.5, reason: 0 });
    await settle();
    expect(h.store.getSnapshot()[0]?.session).toBe(6);
    expect(h.stopped).toEqual([]);
    h.store.update(() => []);
  });

  it("ignores a delayed close from the previous native generation on the same port", async () => {
    const stream = active();
    const demand = { split: true, target: stream.activeTarget };
    const reservation = new ReservedStream(stream.port, demand, {
      async prepareStream() {}, async openStream() { return "src-5007"; },
      async cancelPreparedStream() {}, async getStreamGeneration() { return "new-generation"; },
      async closeStream() {},
    }, async <T>() => ({} as T), new DecoderReservations({ maxInstances: 2 }));
    await reservation.run(demand, async () => stream);
    const h = setup();
    h.store.update(() => [{ ...stream, reservation }]);
    h.emit({ port: 5007, reason: 0, generation: "old-generation" });
    await settle();
    expect(h.store.getSnapshot()).toHaveLength(1);
    expect(reservation.isOpen).toBe(true);
    h.emit({ port: 5007, reason: 0, generation: "new-generation" });
    await settle();
    expect(h.store.getSnapshot()).toEqual([]);
    expect(reservation.isOpen).toBe(false);
  });
});
