import type { RestartedStreamState } from "./launch-stream";
import type {
  AdaptiveQualityState,
  AdaptiveTarget,
} from "./adaptive-resolution";
import type { ResolvedTransport } from "./usb";
import type { EncoderExperimentId } from "./encoder-experiment";
import type { StreamProfile } from "./stream-profile";
import type { UdpStabilitySelection } from "./udp-stability";
import type { ViewerDisplayMetrics } from "./launch-stream";

export interface ActiveStream {
  port: number;
  session: number;
  sourceIndex: number;
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
  mediaTransport: ResolvedTransport;
  viewerIps: string[];
  /** Metrics captured at launch, used by the display-size card. */
  viewerDisplay?: ViewerDisplayMetrics;
  /** Managed host display id encoded in the catalog display name, when present. */
  virtualDisplayId?: string;
  /** HiDPI scale of the active target, when the host confirmed one. */
  scale?: 1 | 2;
  startedAt: number;
}

export type RestoredStream = RestartedStreamState;
