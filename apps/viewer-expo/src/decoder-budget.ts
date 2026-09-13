import type { AdaptiveTarget } from "./adaptive-resolution";
import {
  availableEncoderExperiments,
  resolveEncoderExperimentForStream,
  type EncoderExperimentId,
} from "./encoder-experiment";
import { evenCodecDimension } from "./stream-resolution";

/** Hard ceiling for advisory hardware admission. Runtime DecoderReservations
 * starts at one slot when native hints are absent; advertised values can reduce
 * or raise that trial budget only up to this ceiling. Actual decoder failures
 * retain their reservations until native cleanup acknowledges ownership release.
 */
export const DEFAULT_DECODER_CAPACITY = 4;

/** splitVertical이 점유하는 디코더 인스턴스 수(타일 2개). */
export const SPLIT_DECODER_SLOTS = 2;

/** 일반 창이 점유하는 디코더 인스턴스 수. */
export const SINGLE_DECODER_SLOTS = 1;

export interface AdmissionStream {
  /** splitVertical로 실행 중인 스트림은 디코더 인스턴스 둘을 점유한다. */
  split: boolean;
}

export interface AdmissionRequest {
  /** 어드미션 전에 요청된 인코딩 목표(다운그레이드 판단의 기준점). */
  target: AdaptiveTarget;
  /** 인스턴스 둘을 요구하는 요청: splitVertical 신규 시작 또는 split 승격. */
  split: boolean;
  /**
   * 이 요청이 제자리에서 대체하는 라이브 스트림의 현재 형태(split 승격
   * 경로). 슬롯을 먼저 반납한 뒤 계산하므로 single→split 승격의 순증은
   * +1이다.
   */
  replacing?: AdmissionStream;
}

export type AdmissionAction = "allow" | "downgradeResolution" | "block";

export interface AdmissionPlan {
  allowed: boolean;
  action: AdmissionAction;
  /** 반납(replacing) 반영 후의 현재 점유 슬롯. */
  currentSlots: number;
  /** 이 요청을 수행했을 때의 예상 점유 슬롯. */
  projectedSlots: number;
  capacity: number;
}

/** 스트림 하나가 점유하는 하드웨어 디코더 인스턴스 수. */
export function decoderSlots(stream: AdmissionStream): 1 | 2 {
  return stream.split ? SPLIT_DECODER_SLOTS : SINGLE_DECODER_SLOTS;
}

/** ActiveStream 목록을 어드미션 계산용 형태로 바꾼다. */
export function admissionStreamsFrom(
  streams: readonly { encoderExperiment: string }[],
): AdmissionStream[] {
  return streams.map((stream) => ({
    split: stream.encoderExperiment === "splitVertical",
  }));
}

/**
 * 이 시작 요청이 인스턴스 둘(splitVertical)을 요구하는지 예측한다.
 * startPreparedStream 내부 선택(selectAutomaticEncoderExperiment)을
 * 미러링하되, UDP 여부는 시작 전에 알 수 없으므로(USB 장착이 그 안에서
 * 결정됨) split이 가능한 크기면 auto를 낙관적으로 두 슬롯 요청으로 본다.
 * 비관적 쪽(많이 잡아 보기)으로만 오차가 나므로 초과 허용은 없다.
 */
export function requestedDecoderShape(request: {
  encoderExperiment: EncoderExperimentId;
  width: number;
  height: number;
  advertisedEncoderExperiments?: unknown;
}): boolean {
  const resolved = resolveEncoderExperimentForStream(
    request.encoderExperiment,
    request.advertisedEncoderExperiments,
    request.width,
    request.height,
  );
  if (resolved === "splitVertical") return true;
  if (resolved !== "auto") return false;
  return availableEncoderExperiments(
    request.advertisedEncoderExperiments,
    request.width,
    request.height,
  ).some((experiment) => experiment.id === "splitVertical");
}

/**
 * 순수 어드미션 판정. 결정적이며 부작용이 없다.
 *
 * - split 요청: 두 슬롯이 들어가면 allow, 하나만 들어가면
 *   downgradeResolution(분할을 거절하고 가장 큰 단일 해상도로), 그것도
 *   아니면 block.
 * - 일반 창: 슬롯이 남으면 allow, 넘치면 block. 해상도 강등은 디코더
 *   인스턴스 수를 줄이지 못하므로(슬롯은 split 여부에만 의존) 초과 창을
 *   강등으로 통과시켜 봤자 점유는 그대로 초과한다 — 거절하는 편이
 *   정직하다(errDecoderCapacity). 실제 생명주기 입장은 아래 device-wide
 *   DecoderReservations가 예약하며 이 함수는 기존 순수 정책 API다.
 */
export function planAdmission(
  currentStreams: readonly AdmissionStream[],
  request: AdmissionRequest,
  options: { capacity?: number } = {},
): AdmissionPlan {
  const capacity = Math.max(1, options.capacity ?? DEFAULT_DECODER_CAPACITY);
  const released = request.replacing ? decoderSlots(request.replacing) : 0;
  const currentSlots = Math.max(
    0,
    currentStreams.reduce((sum, stream) => sum + decoderSlots(stream), 0) -
      released,
  );

  const plan = (
    allowed: boolean,
    action: AdmissionAction,
    projectedSlots: number,
  ): AdmissionPlan => ({
    allowed,
    action,
    currentSlots,
    projectedSlots,
    capacity,
  });

  if (request.split) {
    if (currentSlots + SPLIT_DECODER_SLOTS <= capacity) {
      return plan(true, "allow", currentSlots + SPLIT_DECODER_SLOTS);
    }
    // 분할은 거절하되 창 자체는 단일 인스턴스로 들어갈 수 있을 때만
    // 강등으로 허용한다. 승격 경로(replacing 있음)에서는 자기 슬롯을
    // 반납하므로 단일 유지는 항상 들어간다.
    if (currentSlots + SINGLE_DECODER_SLOTS <= capacity) {
      return plan(true, "downgradeResolution", currentSlots + SINGLE_DECODER_SLOTS);
    }
    return plan(false, "block", currentSlots + SINGLE_DECODER_SLOTS);
  }
  if (currentSlots + SINGLE_DECODER_SLOTS <= capacity) {
    return plan(true, "allow", currentSlots + SINGLE_DECODER_SLOTS);
  }
  // 일반 창 초과는 거절한다(위 정책 주석 참조). split 요청의 블록과 같은
  // 형태로, 호출 측의 errDecoderCapacity 흐름을 그대로 탄다.
  return plan(false, "block", currentSlots + SINGLE_DECODER_SLOTS);
}

/**
 * 해상도 강등 사다리 — 화질 프로필 단변 앵커(2160 clarity / 1440 responsive
 * / 1080 latency / 720 하한)를 한 단계씩 내려가고 종횡비와 fps는 유지한다.
 * evenCodecDimension으로 코덱 안전 짝수 정렬을 재사용하고, 업스케일은
 * 하지 않는다. 더 낮은 단계가 없으면 null(이미 최저 단계).
 */
const DOWNGRADE_SHORT_SIDE_STEPS = [2_160, 1_440, 1_080, 720] as const;

export function downgradedStreamTarget(
  target: AdaptiveTarget,
): AdaptiveTarget | null {
  const shortSide = Math.min(target.width, target.height);
  const longSide = Math.max(target.width, target.height);
  const currentIndex = DOWNGRADE_SHORT_SIDE_STEPS.findIndex(
    (step) => shortSide >= step,
  );
  if (currentIndex < 0 || currentIndex + 1 >= DOWNGRADE_SHORT_SIDE_STEPS.length) {
    return null;
  }
  const nextShortSide = DOWNGRADE_SHORT_SIDE_STEPS[currentIndex + 1];
  const scale = nextShortSide / shortSide;
  const nextLongSide = evenCodecDimension(longSide * scale);
  const landscape = target.width >= target.height;
  return {
    width: landscape ? nextLongSide : nextShortSide,
    height: landscape ? nextShortSide : nextLongSide,
    fps: target.fps,
  };
}

/** Native Task7 hint: instance count and aggregate pixels/second are independent.
 * Missing/invalid instance hints admit one slot; advertised limits never exceed four.
 */
export interface DecoderCapabilityHint {
  codecName?: string;
  maxInstances?: number;
  maxPixelRate?: number;
  maxInstancePixelRate?: number;
}
export interface DecoderDemand extends AdmissionStream { target: AdaptiveTarget }
export interface DecoderLease { readonly id: symbol }
interface Reservation {
  demand: DecoderDemand;
  busy?: Promise<unknown>;
  closing?: Promise<void>;
  stopped: boolean;
}

/** Device-wide admission owner. React render/status snapshots never own slots.
 * Leases and operation promises preserve identity through late completion,
 * cancellation, reconfiguration, failed cleanup, and component remounts.
 */
export class DecoderReservations {
  private readonly entries = new Map<DecoderLease, Reservation>();
  constructor(private hint: DecoderCapabilityHint = {}) {}

  setCapability(hint: DecoderCapabilityHint): void { this.hint = hint; }

  async refreshCapability(launcher: {getDecoderCapabilityHint?(): Promise<DecoderCapabilityHint>}): Promise<void> {
    try { this.setCapability(await launcher.getDecoderCapabilityHint?.() ?? {}); }
    catch { this.setCapability({}); }
  }

  private capacity(): number {
    const value = this.hint.maxInstances;
    return value !== undefined && Number.isFinite(value) && value >= 1
      ? Math.min(DEFAULT_DECODER_CAPACITY, Math.floor(value)) : 1;
  }

  private fits(demand: DecoderDemand, replacing?: DecoderLease): boolean {
    if (demand.target.fps > 90) return false;
    if (![demand.target.width, demand.target.height, demand.target.fps].every((value) => Number.isFinite(value) && value > 0)) return false;
    const perInstanceLimit = this.hint.maxInstancePixelRate;
    const instanceFits = (candidate: DecoderDemand): boolean => {
      const rate = candidate.target.width * candidate.target.height * candidate.target.fps / decoderSlots(candidate);
      return !(perInstanceLimit !== undefined && Number.isFinite(perInstanceLimit) && perInstanceLimit > 0 && rate > perInstanceLimit);
    };
    if (!instanceFits(demand)) return false;
    let slots = decoderSlots(demand);
    let pixels = demand.target.width * demand.target.height * demand.target.fps;
    for (const [lease, entry] of this.entries) {
      if (lease === replacing) continue;
      slots += decoderSlots(entry.demand);
      pixels += entry.demand.target.width * entry.demand.target.height * entry.demand.target.fps;
    }
    const pixelLimit = this.hint.maxPixelRate;
    return slots <= this.capacity() &&
      !(pixelLimit !== undefined && Number.isFinite(pixelLimit) && pixelLimit > 0 && pixels > pixelLimit);
  }

  /** Returns a smaller single target only when both independent budgets fit. */
  plan(demand: DecoderDemand, replacing?: DecoderLease): DecoderDemand | null {
    if (this.fits(demand, replacing)) return demand;
    let target = downgradedStreamTarget(demand.target);
    while (target) {
      const candidate = {split: false, target};
      if (this.fits(candidate, replacing)) return candidate;
      target = downgradedStreamTarget(target);
    }
    return null;
  }

  reserve(demand: DecoderDemand): DecoderLease {
    if (!this.fits(demand)) throw new Error('Decoder capacity exhausted');
    const lease = {id: Symbol('decoder lease')};
    this.entries.set(lease, {demand, stopped:false});
    return lease;
  }

  isOpen(lease: DecoderLease): boolean {
    const entry = this.entries.get(lease);
    return entry !== undefined && !entry.stopped;
  }

  async run<T extends DecoderDemand>(
    lease: DecoderLease, demand: DecoderDemand, work: () => Promise<T>,
  ): Promise<T> {
    const entry = this.entries.get(lease);
    if (!entry || entry.stopped || entry.busy) throw new Error('Decoder operation is no longer available');
    // A downgrade cannot lend its old slots/pixel rate while native replacement
    // is still pending. Same-port operations are exclusive, including restores.
    const previous = entry.demand;
    const held = {
      split: previous.split || demand.split,
      target: previous.target.width * previous.target.height * previous.target.fps >
        demand.target.width * demand.target.height * demand.target.fps ? previous.target : demand.target,
    };
    if (!this.fits(held, lease)) throw new Error('Decoder capacity exhausted');
    entry.demand = held;
    // Publish ownership BEFORE invoking work (which can synchronously reenter).
    let complete!: () => void;
    entry.busy = new Promise<void>((resolve) => { complete = resolve; });
    try {
      const accepted = await work();
      if (decoderSlots(accepted) > decoderSlots(held) ||
          accepted.target.width * accepted.target.height * accepted.target.fps >
          held.target.width * held.target.height * held.target.fps) {
        throw new Error('Native decoder exceeded its reservation');
      }
      entry.demand = accepted;
      return accepted;
    } catch (error) {
      // Keep the maximum on failure: native cleanup may still be unresolved.
      throw error;
    } finally {
      entry.busy = undefined;
      complete();
    }
  }

  close(lease: DecoderLease, cleanup: () => Promise<void>): Promise<void> {
    const entry = this.entries.get(lease);
    if (!entry) return Promise.resolve();
    if (entry.closing) return entry.closing;
    entry.stopped = true;
    const operation = (async () => {
      await entry.busy;
      await cleanup();
      // Delete this exact incarnation only after acknowledged resource cleanup.
      if (this.entries.get(lease) === entry) this.entries.delete(lease);
    })();
    entry.closing = operation;
    void operation.finally(() => { entry.closing = undefined; }).catch(() => undefined);
    return operation;
  }
}

/** Survives catalog unmounts and Host selections while native resources live. */
export const deviceDecoderReservations = new DecoderReservations();
