import type { RestartedStreamState } from "./launch-stream";
import type { ResolvedTransport } from "./usb";
import type { EncoderExperimentId } from "./encoder-experiment";
import type { StreamProfile } from "./stream-profile";
import type { UdpStabilitySelection } from "./udp-stability";

export interface ActiveStream {
  port: number;
  session: number;
  sourceIndex: number;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
  contentMode: StreamProfile["contentMode"];
  encoderExperiment: EncoderExperimentId;
  udpStability?: UdpStabilitySelection;
  showFps?: boolean;
  mediaTransport: ResolvedTransport;
  viewerIps: string[];
  startedAt: number;
}

export type RestoredStream = RestartedStreamState;
