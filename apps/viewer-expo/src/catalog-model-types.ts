import type { ReservedStream } from "./reserved-stream";
import type { RestartedStreamState } from "./launch-stream";
import type {
  AdaptiveQualityState,
  AdaptiveTarget,
} from "./adaptive-resolution";
import type { ResolvedTransport } from "./usb";
import type { EncoderExperimentId } from "./encoder-experiment";
import type { StreamProfile } from "./stream-profile";
import type { UdpStabilitySelection } from "./udp-stability";

export interface ActiveStream {
  /** Opaque local ownership; never serialized to the Host. */
  reservation?: ReservedStream;
  port: number;
  session: number;
  sourceIndex: number;
  sourceId?: string;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  sourceTarget: AdaptiveTarget;
  activeTarget: AdaptiveTarget;
  fallbackTarget: AdaptiveTarget | null;
  qualityState: AdaptiveQualityState;
  captureBackend: string;
  contentMode: StreamProfile["contentMode"];
  encoderExperiment: EncoderExperimentId;
  udpStability?: UdpStabilitySelection;
  showFps?: boolean;
  localCursor?: boolean;
  localAudio?: boolean;
  /** Requested codec admitted by native module; runtime fallback is separately native. */
  opusAudio?: boolean;
  balancedPresentation?: boolean;
  mediaTransport: ResolvedTransport;
  viewerIps: string[];
  /** Viewer-generated session media key (base64url). Seals the media path;
   * reused verbatim when this session is reconfigured. */
  mediaKey: string;
  startedAt: number;
}

export type RestoredStream = RestartedStreamState;
