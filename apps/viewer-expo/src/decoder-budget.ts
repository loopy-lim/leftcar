import type { AdaptiveTarget } from "./adaptive-resolution";
import {
  availableEncoderExperiments,
  resolveEncoderExperimentForStream,
  type EncoderExperimentId,
} from "./encoder-experiment";
import { evenCodecDimension } from "./stream-resolution";

/**
 * 하드웨어 디코더 동시 인스턴스 예산 (performance review M4/R8).
 *
 * splitVertical은 타일마다 디코더 인스턴스를 하나씩, 총 두 개를 점유하고
 * 일반 창은 하나를 점유한다. 네이티브 프로브(SplitDecoderCapability)는
 * split 타일 크기(1920×2160@60)에서 maxSupportedInstances ≥ 2만 보장할 뿐
 * 기기별 상한을 JS로 노출하지 않으므로, v1에서는 문서에 기록된 실패
 * 한계점(분할 1개 + 창 2개 = 4 인스턴스에서 생성 실패)에 맞춘 보수적
 * 상수 4를 쓴다.
 *
 * 네이티브 확장 경로: SplitDecoderCapability가 선택된 코덱의
 * maxSupportedInstances(및 임의 크기 지원 여부)를 반환하고
 * StreamLauncherModule이 이를 조회하는 @ReactMethod로 노출하면, JS는
 * 연결 시 한 번 읽어 planAdmission의 capacity 옵션으로 넘기면 된다.
 * 모듈이 capacity를 주입 가능하게 설계한 이유다.
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
 *   정직하다(errDecoderCapacity). 남은 v1 한계: 용량은 고정 상수
 *   DEFAULT_DECODER_CAPACITY=4(네이티브 프로브 미노출)이며, 동시
 *   openDisplay 경쟁(두 호출이 모두 등록 전에 검사를 통과)은 슬롯 예약
 *   없이는 그대로 허용된다. 이 두 한계는 네이티브 상한 노출 + 등록 시점
 *   재검토로 조일 수 있다.
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
