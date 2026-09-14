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

  run<T extends StartedStream>(
    demand: DecoderDemand,
    work: (launcher: StreamLauncher) => Promise<T>,
  ): Promise<T> {
    return this.pool
      .run(this.lease, demand, async () => {
        this.attempted = true;
        this.nativeGeneration = undefined;
        // TurboModule methods can live on a lazy prototype until first access.
        // Resolve them explicitly and preserve the native module as receiver.
        const ownedLauncher: StreamLauncher = {
          getDecoderCapabilityHint: this.launcher.getDecoderCapabilityHint?.bind(this.launcher),
          getLocalIpv4Addresses: this.launcher.getLocalIpv4Addresses?.bind(this.launcher),
          getStreamGeneration: this.launcher.getStreamGeneration?.bind(this.launcher),
          closeStream: this.launcher.closeStream?.bind(this.launcher),
          cancelPreparedStream: this.launcher.cancelPreparedStream.bind(this.launcher),
          setBalancedPresentation: this.launcher.setBalancedPresentation?.bind(this.launcher),
          setCursorStream: this.launcher.setCursorStream?.bind(this.launcher),
          setOpusAudio: this.launcher.setOpusAudio?.bind(this.launcher),
          setAudioStream: this.launcher.setAudioStream?.bind(this.launcher),
          setWindowAspectRatio: this.launcher.setWindowAspectRatio?.bind(this.launcher),
          isXrWindowRatioSupported: this.launcher.isXrWindowRatioSupported?.bind(this.launcher),
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
        const started = await work(ownedLauncher);
        this.latest = started;
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
      if (this.latest)
        await this.request("stopStream", {
          session: this.latest.session,
        }).catch(() => undefined);
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
