import {
  deviceDecoderReservations,
  type DecoderDemand,
  type DecoderLease,
  type DecoderReservations,
} from "./decoder-budget";
import type { EncoderExperimentId } from "./encoder-experiment";
import type {
  StartedStream,
  StreamControlRequest,
  StreamLauncher,
} from "./launch-stream";

const abandonedReservations = new Set<ReservedStream>();

/** Refresh retries cleanup without an active stream action, including failed starts. */
export async function retryAbandonedDecoderCleanup(): Promise<void> {
  await Promise.all(
    [...abandonedReservations].map((reservation) => reservation.close()),
  );
}

/** One native port incarnation, including pending work and failed teardown. */
export class ReservedStream {
  readonly lease: DecoderLease;
  private latest?: StartedStream;
  private nativeGeneration?: string;
  private latestControlRequest?: StreamControlRequest;
  private attempted = false;
  private preparedExperiment: EncoderExperimentId = "auto";
  constructor(
    readonly port: number,
    demand: DecoderDemand,
    private readonly launcher: StreamLauncher,
    private readonly request: StreamControlRequest,
    readonly pool: DecoderReservations = deviceDecoderReservations,
    readonly controlRequest: StreamControlRequest = request,
  ) {
    this.lease = pool.reserve(demand);
  }

  get isOpen(): boolean {
    return this.pool.isOpen(this.lease);
  }

  get generation(): string | undefined { return this.nativeGeneration; }

  run<T extends StartedStream>(
    demand: DecoderDemand,
    work: (launcher: StreamLauncher) => Promise<T>,
    controlRequest: StreamControlRequest = this.controlRequest,
  ): Promise<T> {
    return this.pool
      .run(this.lease, demand, async () => {
        this.attempted = true;
        this.nativeGeneration = undefined;
        const guarded: Partial<StreamLauncher> = {
          prepareStream: async (...args) => {
            if (!this.isOpen)
              throw new Error("Stream was stopped before preparation");
            this.preparedExperiment = args[3];
            await this.launcher.prepareStream(...args);
            if (!this.isOpen)
              throw new Error("Stream was stopped during preparation");
          },
          openStreamWithPresentation: this.launcher.openStreamWithPresentation ? (...args) => {
            if (!this.isOpen) return Promise.reject(new Error("Stream was stopped before opening"));
            return this.launcher.openStreamWithPresentation!(...args);
          } : undefined,
          openStream: (...args) => {
            if (!this.isOpen)
              return Promise.reject(
                new Error("Stream was stopped before opening"),
              );
            return this.launcher.openStream(...args);
          },
        };
        // NativeModules methods need not be enumerable. Spreading the module
        // can silently drop cancelPreparedStream and replace a real recovery
        // error with "undefined is not a function" during cleanup.
        const ownedLauncher = new Proxy(this.launcher, {
          get(target, key) {
            if (Object.prototype.hasOwnProperty.call(guarded, key)) {
              return Reflect.get(guarded, key);
            }
            const value = Reflect.get(target, key, target);
            return typeof value === "function" ? value.bind(target) : value;
          },
        });
        const started = await work(ownedLauncher);
        this.latest = started;
        // A retained native window can be restored after the same Host is
        // selected again. Its new session belongs to that successful request,
        // while the original launch route correctly remains cancelled.
        this.latestControlRequest = controlRequest;
        if (this.launcher.getStreamGeneration) {
          this.nativeGeneration = await this.launcher.getStreamGeneration(
            `src-${this.port}`,
          );
        }
        return {
          ...started,
          split: started.encoderExperiment === "splitVertical",
          target: {
            width: started.width ?? demand.target.width,
            height: started.height ?? demand.target.height,
            fps: started.fps ?? demand.target.fps,
          },
        };
      })
      .then((accepted) => {
        if (!this.isOpen)
          throw new Error("Stream was stopped while native work was pending");
        return accepted;
      });
  }

  retainForCleanupRetry(): void {
    abandonedReservations.add(this);
  }

  abandon(): void {
    this.retainForCleanupRetry();
    void this.close().catch(() => undefined);
  }

  async close(): Promise<void> {
    await this.pool.close(this.lease, async () => {
      // Host status is best effort; native acknowledgment owns slot release.
      if (this.latest) {
        const args = { session: this.latest.session };
        // Recovery may replace the control socket while this reservation
        // remains alive. Prefer its selection-fenced, reconnecting route.
        // If the user selected another Host, only the original client may
        // receive cleanup; never send this session id to the newly selected one.
        const request = this.latestControlRequest ?? this.controlRequest;
        await request("stopStream", args).catch(async (error) => {
          console.warn("[leftcar] Host stop request failed", error instanceof Error ? error.message : String(error));
          if (request !== this.request)
            await this.request("stopStream", args).catch(() => undefined);
        });
      }
      if (this.launcher.getStreamGeneration && this.launcher.closeStream) {
        const generation =
          this.nativeGeneration ??
          (await this.launcher.getStreamGeneration(`src-${this.port}`));
        if (generation)
          await this.launcher.closeStream(`src-${this.port}`, generation);
      } else if (this.attempted) {
        throw new Error(
          "Decoder cleanup cannot be confirmed. Update the Viewer and retry stopping this stream.",
        );
      }
      // Cancel the actual last preparation (including native fallback), not
      // unrelated transports. Failed cancellation retains this exact lease.
      await this.launcher.cancelPreparedStream(
        this.port,
        this.preparedExperiment,
      );
    });
    abandonedReservations.delete(this);
  }
}
