import { describe, expect, it } from "vitest";
import { DecoderReservations } from "./decoder-budget";
const target = { width: 1920, height: 1080, fps: 60 };
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
describe("decoder reservations", () => {
  it("counts pending starts synchronously and never buys instances by lowering pixels", async () => {
    const pool = new DecoderReservations({maxInstances: 4});
    const pending = deferred<{ split: boolean; target: typeof target }>();
    const leases = Array.from({ length: 4 }, () =>
      pool.reserve({ split: false, target }),
    );
    const run = pool.run(
      leases[0],
      { split: false, target },
      () => pending.promise,
    );
    expect(() =>
      pool.reserve({
        split: false,
        target: { width: 720, height: 480, fps: 30 },
      }),
    ).toThrow();
    pending.resolve({ split: false, target });
    await run;
    expect(() => pool.reserve({ split: false, target })).toThrow();
  });
  it("holds canceled native work through late completion and actual cleanup", async () => {
    const pool = new DecoderReservations({ maxInstances: 2 });
    const lease = pool.reserve({ split: true, target });
    const pending = deferred<{ split: boolean; target: typeof target }>();
    const cleanup = deferred<void>();
    const run = pool.run(lease, { split: true, target }, () => pending.promise);
    const stop = pool.close(lease, () => cleanup.promise);
    expect(() => pool.reserve({ split: false, target })).toThrow();
    pending.resolve({ split: true, target });
    await run;
    expect(() => pool.reserve({ split: false, target })).toThrow();
    cleanup.resolve();
    await stop;
    const fresh = pool.reserve({ split: true, target });
    await pool.close(lease, async () => {
      throw new Error("old cleanup must not run");
    });
    expect(() => pool.reserve({ split: false, target })).toThrow();
    await pool.close(fresh, async () => {});
  });
  it("retains the old two-slot cost until downgrade settles and preserves failed cleanup for retry", async () => {
    const pool = new DecoderReservations({ maxInstances: 2 });
    const lease = pool.reserve({ split: true, target });
    const pending = deferred<{ split: boolean; target: typeof target }>();
    const run = pool.run(
      lease,
      { split: false, target },
      () => pending.promise,
    );
    expect(() => pool.reserve({ split: false, target })).toThrow();
    pending.resolve({ split: false, target });
    await run;
    const other = pool.reserve({ split: false, target });
    await expect(
      pool.close(lease, async () => {
        throw new Error("decoder cleanup timed out");
      }),
    ).rejects.toThrow("timed out");
    expect(() => pool.reserve({ split: false, target })).toThrow();
    await pool.close(lease, async () => {});
    await pool.close(other, async () => {});
  });
  it("keeps named instance capacity independent from pixel-rate capacity", () => {
    const pool = new DecoderReservations({
      codecName: "codec.example",
      maxInstances: 2,
      maxPixelRate: 1920 * 1080 * 60,
    });
    pool.reserve({ split: false, target });
    expect(() => pool.reserve({ split: false, target })).toThrow();
    const smaller = new DecoderReservations({
      maxInstances: 1,
      maxPixelRate: 1e12,
    });
    smaller.reserve({
      split: false,
      target: { width: 720, height: 480, fps: 30 },
    });
    expect(() =>
      smaller.reserve({
        split: false,
        target: { width: 720, height: 480, fps: 30 },
      }),
    ).toThrow();
  });
});

describe("advertised codec hint boundary", () => {
  const split = {split:true, target:{width:3840,height:2160,fps:60}};
  it("loads a named two-instance hint without mistaking per-instance rate for aggregate", async () => {
    const pool = new DecoderReservations();
    expect(() => pool.reserve(split)).toThrow();
    await pool.refreshCapability({getDecoderCapabilityHint:async () => ({codecName:"c2.qti.avc.decoder",maxInstances:2,maxInstancePixelRate:1920*2160*60})});
    const lease = pool.reserve(split);
    expect(pool.isOpen(lease)).toBe(true);
    expect(() => pool.reserve({split:false,target})).toThrow();
  });
  it("keeps real aggregate policy limits independent and does not gain slots from lying hints", async () => {
    const pool = new DecoderReservations({maxInstances:2,maxInstancePixelRate:1920*2160*60,maxPixelRate:1920*2160*60});
    expect(() => pool.reserve(split)).toThrow();
    await pool.refreshCapability({getDecoderCapabilityHint:async () => ({maxInstances:999})});
    for(let i=0;i<4;i++) pool.reserve({split:false,target});
    expect(() => pool.reserve({split:false,target})).toThrow();
  });
  it("failed probes shrink admission without discarding live and failed-cleanup leases", async () => {
    const pool = new DecoderReservations({maxInstances:2});
    const lease=pool.reserve(split);
    await pool.refreshCapability({getDecoderCapabilityHint:async () => {throw new Error("probe");}});
    expect(pool.isOpen(lease)).toBe(true);
    expect(() => pool.reserve({split:false,target})).toThrow();
    await expect(pool.close(lease,async()=>{throw new Error("actual fallback release incomplete");})).rejects.toThrow();
    expect(() => pool.reserve({split:false,target})).toThrow();
    await pool.close(lease,async()=>{});
    expect(() => pool.reserve(split)).toThrow();
    pool.reserve({split:false,target});
  });
});

it("120Hz display testing never expands the stream FPS admission ceiling", () => {
  const pool = new DecoderReservations({maxInstances:4});
  expect(() => pool.reserve({split:false,target:{...target,fps:120}})).toThrow();
});
