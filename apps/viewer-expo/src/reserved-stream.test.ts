import { describe, expect, it } from "vitest";
import { DecoderReservations } from "./decoder-budget";
import { ReservedStream, retryAbandonedDecoderCleanup } from "./reserved-stream";
import {
  startPreparedStream,
  reconfigurePreparedStream,
  type StreamLauncher,
} from "./launch-stream";
import { setRandomSource } from "./secure-channel";
import type { ControlClient } from "./control";
import type { ActiveStream } from "./catalog-model-types";
setRandomSource((n) => new Uint8Array(n).fill(7));
const target = { width: 1920, height: 1080, fps: 60 };
const args = {
  sourceIndex: 0,
  viewerPort: 5003,
  ...target,
  captureBackend: "screenCaptureKit",
  mediaTransport: "udp",
  encoderExperiment: "auto" as const,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function io() {
  const calls: string[] = [];
  const launcher: StreamLauncher = {
    async prepareStream() {
      calls.push("prepare");
    },
    async openStream() {
      calls.push("open");
      return "src-5003";
    },
    async cancelPreparedStream() {
      calls.push("cancel");
    },
    async getStreamGeneration() {
      return "generation-1";
    },
    async closeStream(id, generation) {
      calls.push(`close:${id}:${generation}`);
    },
  };
  const control = {
    request: async (command: string) => {
      calls.push(command);
      return { session: 17, ...target };
    },
    close() {},
  } as ControlClient;
  return { launcher, control, calls };
}
describe("reserved production launch lifecycle", () => {
  it("closes the replacement through the selection that successfully restored it", async () => {
    const { launcher } = io();
    const stopped: number[] = [];
    let originalSelectionActive = true;
    const original = async <T>(): Promise<T> => {
      if (!originalSelectionActive) throw new Error("Host selection changed");
      return undefined as T;
    };
    const restored = async <T>(_command: string, payload?: unknown): Promise<T> => {
      stopped.push((payload as { session: number }).session);
      return undefined as T;
    };
    const reservation = new ReservedStream(5003, { split: false, target }, launcher,
      original, new DecoderReservations({ maxInstances: 1 }));
    const started = { session: 20, ...target, mediaKey: "fixture", viewerIps: [],
      mediaTransport: "udp" as const, encoderExperiment: "auto" as const };
    await reservation.run({ split: false, target }, async () => started);
    originalSelectionActive = false;
    await reservation.run({ split: false, target }, async () => ({ ...started, session: 21 }), restored);
    await reservation.close();
    expect(stopped).toEqual([21]);
  });

  it("stops a recovered session through the current control connection", async () => {
    const { launcher } = io();
    const stopped: number[] = [];
    const obsolete = async () => { throw new Error("control connection closed"); };
    const current = async <T>(command: string, payload?: unknown): Promise<T> => {
      if (command === "stopStream") stopped.push((payload as { session: number }).session);
      return undefined as T;
    };
    const reservation = new ReservedStream(5003, { split: false, target }, launcher,
      obsolete, new DecoderReservations({ maxInstances: 1 }), current);
    await reservation.run({ split: false, target }, async () => ({ session: 18, ...target, mediaKey: "fixture", viewerIps: [],
      mediaTransport: "udp" as const, encoderExperiment: "auto" as const }));
    await reservation.close();
    expect(stopped).toEqual([18]);
  });

  it("uses the original Host for cleanup when selection has changed", async () => {
    const { launcher } = io();
    const stopped: number[] = [];
    const original = async <T>(_command: string, payload?: unknown): Promise<T> => {
      stopped.push((payload as { session: number }).session);
      return undefined as T;
    };
    const changedSelection = async () => { throw new Error("Host selection changed"); };
    const reservation = new ReservedStream(5003, { split: false, target }, launcher,
      original, new DecoderReservations({ maxInstances: 1 }), changedSelection);
    await reservation.run({ split: false, target }, async () => ({ session: 18, ...target, mediaKey: "fixture", viewerIps: [],
      mediaTransport: "udp" as const, encoderExperiment: "auto" as const }));
    await reservation.close();
    expect(stopped).toEqual([18]);
  });

  it("retains native methods that are not enumerable and preserves the actual preparation failure", async () => {
    const calls: string[] = [];
    class NativeLauncher implements StreamLauncher {
      async getLocalIpv4Addresses() { calls.push("addresses"); return ["192.168.0.19"]; }
      async prepareStream() { throw new Error("receiver unavailable"); }
      async openStream() { return "src-5003"; }
      async cancelPreparedStream() { calls.push("cancel"); }
    }
    const { control } = io();
    const reservation = new ReservedStream(5003, { split: false, target }, new NativeLauncher(),
      control.request.bind(control), new DecoderReservations({ maxInstances: 1 }));
    await expect(reservation.run({ split: false, target }, (owned) => startPreparedStream({
      launcher: owned, control, host: "127.0.0.1", advertisedEncoderExperiments: [], args,
    }))).rejects.toThrow("receiver unavailable");
    expect(calls).toEqual(["addresses", "cancel"]);
  });

  it.each(["legacy", "presentation"] as const)("cancel during %s native open counts resources until late completion and cleanup acknowledge", async (method) => {
    const pool = new DecoderReservations({ maxInstances: 1 });
    const { launcher, control, calls } = io();
    const prepared = deferred<void>();
    const closed = deferred<void>();
    const entered = deferred<void>();
    const open = async () => {
      calls.push("open");
      entered.resolve();
      await prepared.promise;
      return "src-5003";
    };
    if (method === "presentation") launcher.openStreamWithPresentation = open;
    else launcher.openStream = open;
    launcher.closeStream = async (id, generation) => {
      calls.push(`close:${id}:${generation}`);
      await closed.promise;
    };
    const reservation = new ReservedStream(
      5003,
      { split: false, target },
      launcher,
      control.request.bind(control),
      pool,
    );
    const run = reservation.run({ split: false, target }, (ownedLauncher) =>
      startPreparedStream({
        launcher: ownedLauncher,
        control,
        host: "127.0.0.1",
        advertisedEncoderExperiments: [],
        args,
      }),
    );
    await entered.promise;
    const stop = reservation.close();
    expect(pool.plan({ split: false, target })).toBeNull();
    prepared.resolve();
    await expect(run).rejects.toThrow("stopped");
    await Promise.resolve();
    await Promise.resolve();
    expect(reservation.isOpen).toBe(false);
    expect(pool.plan({ split: false, target })).toBeNull();
    closed.resolve();
    await stop;
    expect(calls).toContain("close:src-5003:generation-1");
    expect(calls.indexOf("open")).toBeLessThan(calls.indexOf("stopStream"));
    expect(pool.plan({ split: false, target })).not.toBeNull();
  });
  it("unavailable old-module cleanup cannot silently free a live decoder", async () => {
    const pool = new DecoderReservations({ maxInstances: 1 });
    const { launcher, control } = io();
    delete launcher.closeStream;
    delete launcher.getStreamGeneration;
    const reservation = new ReservedStream(
      5003,
      { split: false, target },
      launcher,
      control.request.bind(control),
      pool,
    );
    await reservation.run({ split: false, target }, (ownedLauncher) =>
      startPreparedStream({
        launcher: ownedLauncher,
        control,
        host: "127.0.0.1",
        advertisedEncoderExperiments: [],
        args,
      }),
    );
    await expect(reservation.close()).rejects.toThrow(
      "cleanup cannot be confirmed",
    );
    expect(pool.plan({ split: false, target })).toBeNull();
  });
  it("unexpected Host split acceptance cannot allocate outside a single reservation", async () => {
    const { launcher, control, calls } = io();
    launcher.prepareStream = async (_port, _host, _transport, experiment) => {
      calls.push(`prepare:${experiment}`);
    };
    control.request = async () =>
      ({ session: 17, ...target, encoderExperiment: "splitVertical" }) as never;
    const active: ActiveStream = {
      port: 5003,
      session: 17,
      encoderExperiment: "auto",
      mediaTransport: "udp",
      mediaKey: "key",
      viewerIps: [],
      sourceName: "Screen",
      sourceIndex: 0,
      ...target,
      sourceTarget: target,
      activeTarget: target,
      fallbackTarget: null,
      qualityState: "native",
      captureBackend: "screenCaptureKit",
      contentMode: "interactive",
      startedAt: 0,
    };
    const input = {
      control,
      launcher,
      host: "127.0.0.1",
      active,
      target,
      qualityState: "native" as const,
      decoderReservation: { split: false, target },
    };
    await expect(reconfigurePreparedStream(input)).rejects.toThrow(
      "reservation",
    );
    expect(calls).not.toContain("prepare:splitVertical");
  });
  it("Host enlargement is rejected before opening a decoder outside its pixel reservation", async () => {
    const { launcher, control, calls } = io();
    const small = { width: 640, height: 480, fps: 30 };
    const input = {
      launcher,
      control,
      host: "127.0.0.1",
      advertisedEncoderExperiments: [],
      args: { ...args, ...small },
      decoderReservation: { split: false, target: small },
    };
    await expect(startPreparedStream(input)).rejects.toThrow("reservation");
    expect(calls).not.toContain("open");
  });
  it("failed cleanup can be handed to Refresh without releasing its occupied slot", async () => {
    const pool = new DecoderReservations({ maxInstances: 1 });
    const { launcher, control } = io();
    let attempts = 0;
    launcher.closeStream = async () => {
      if (++attempts === 1) throw new Error("cleanup incomplete");
    };
    const reservation = new ReservedStream(5003, { split: false, target }, launcher, control.request, pool);
    await expect(reservation.close()).rejects.toThrow("cleanup incomplete");
    reservation.retainForCleanupRetry();
    expect(pool.plan({ split: false, target })).toBeNull();
    await retryAbandonedDecoderCleanup();
    expect(attempts).toBe(2);
    expect(pool.plan({ split: false, target })).toEqual({ split: false, target });
  });

});

it("new native presentation open is fenced when cancellation lands during Host start", async () => {
  const pool = new DecoderReservations({maxInstances:1});
  const {launcher, control, calls}=io();
  const entered=deferred<void>();
  const released=deferred<void>();
  launcher.openStreamWithPresentation=async()=>{calls.push("new-open");return "src-5003";};
  const request=control.request;
  control.request=(async(command:string)=>{
    if(command==="startStream") {entered.resolve();await released.promise;}
    return request(command);
  }) as ControlClient["request"];
  const reservation=new ReservedStream(5003,{split:false,target},launcher,control.request.bind(control),pool);
  const run=reservation.run({split:false,target},owned=>startPreparedStream({launcher:owned,control,host:"127.0.0.1",advertisedEncoderExperiments:[],args:{...args,balancedPresentation:true}}));
  await entered.promise;
  const stop=reservation.close();
  released.resolve();
  await expect(run).rejects.toThrow("stopped");
  await stop;
  expect(calls).not.toContain("new-open");
});
