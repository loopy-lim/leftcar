/**
 * 뷰어 진단 레코드 스키마와 형식화(docs/07 §16·§18).
 *
 * 세션 레코드만 TS가 만든다. sample·crash 레코드는 JS가 죽은 뒤에도
 * 기록되어야 하므로 Kotlin 측(`DiagnosticsStore`·`DiagnosticsSampler`·
 * `CrashMarker`)이 같은 스키마로 직접 직렬화하며 JVM 테스트가 형식을
 * 고정한다. 저장 상한·회전 상수는 양쪽에 같은 값으로 존재하고 각자의
 * 테스트가 검증한다.
 */

/** §16 저장 규격과 샘플 간격. Kotlin 측 상수와 동일해야 한다. */
export const SAMPLE_INTERVAL_MS = 15_000;
export const MAX_RECORD_BYTES = 1024;
export const MAX_FILE_BYTES = 5 * 1024 * 1024;
export const BACKUP_GENERATIONS = 1;
/** elapsedRealtime이 이 값보다 작으면 모듈 init 시점을 기기 부팅 직후로 본다. */
export const BOOT_DETECTION_UPTIME_MS = 120_000;

export type DiagnosticsSessionEvent =
  | "session_start"
  | "foreground"
  | "background"
  | "stream_start"
  | "stream_stop"
  | "thermal_cap"
  | "thermal_cap_cleared";

export interface DiagnosticsSessionRecord {
  /** epoch ms — 사용자 기록과의 대조용. */
  ts: number;
  /** SystemClock.elapsedRealtime — 재부팅 증명용. */
  upMs: number;
  event: DiagnosticsSessionEvent;
  /** BuildConfig.VERSION_NAME 등. 허용 문자셋 밖은 "unknown" 치환. */
  appVersion?: string;
  osApi?: number;
  bootDetected?: boolean;
  /** 화질 프로필·인코더 실험 ID 같은 안정적 식별자. */
  profile?: string;
  decoderSlots?: number;
  /** thermal_cap 진입 시 상태(PowerManager 0..6)·용량 상한·시작 단변 상한. */
  thermalStatus?: number;
  capacityCap?: number;
  startShortSideCap?: number;
}

const SESSION_EVENTS = new Set<string>([
  "session_start",
  "foreground",
  "background",
  "stream_start",
  "stream_stop",
  "thermal_cap",
  "thermal_cap_cleared",
]);

/** 식별자 허용 문자셋: 단어 문자·점·대시·덧셈 기호. 공백·콜론·슬래시는 없다. */
const SAFE_IDENTIFIER = /^[\w.\-+]{1,40}$/;

function safeIdentifier(value: string): string {
  return SAFE_IDENTIFIER.test(value) ? value : "unknown";
}

function clockField(value: number, name: string): number {
  if (!Number.isFinite(value) || value < 0) {
    throw new TypeError(`diagnostics clock field ${name} must be a finite non-negative number`);
  }
  return Math.round(value);
}

/**
 * 세션 레코드 한 줄. 키 순서는 스키마 고정 순서다. 문자열 필드는 허용
 * 문자셋 밖에서 "unknown"으로 치환되므로 제목·IP·토큰·경로가 흘러들어도
 * 레코드에 남지 않는다(§16 denylist).
 */
export function formatSession(record: DiagnosticsSessionRecord): string {
  if (!SESSION_EVENTS.has(record.event)) {
    throw new TypeError(`diagnostics unknown session event: ${String(record.event)}`);
  }
  const payload: Record<string, unknown> = {
    ts: clockField(record.ts, "ts"),
    up_ms: clockField(record.upMs, "upMs"),
    kind: "session",
    event: record.event,
  };
  if (record.appVersion !== undefined) {
    payload.app_version = safeIdentifier(record.appVersion);
  }
  if (record.osApi !== undefined && Number.isInteger(record.osApi) && record.osApi >= 1) {
    payload.os_api = record.osApi;
  }
  if (record.bootDetected !== undefined) {
    payload.boot_detected = record.bootDetected;
  }
  if (record.profile !== undefined) {
    payload.profile = safeIdentifier(record.profile);
  }
  if (
    record.decoderSlots !== undefined &&
    Number.isInteger(record.decoderSlots) &&
    record.decoderSlots >= 1 &&
    record.decoderSlots <= 8
  ) {
    payload.decoder_slots = record.decoderSlots;
  }
  if (
    record.thermalStatus !== undefined &&
    Number.isInteger(record.thermalStatus) &&
    record.thermalStatus >= 0 &&
    record.thermalStatus <= 6
  ) {
    payload.thermal_status = record.thermalStatus;
  }
  if (
    record.capacityCap !== undefined &&
    Number.isInteger(record.capacityCap) &&
    record.capacityCap >= 1 &&
    record.capacityCap <= 8
  ) {
    payload.capacity_cap = record.capacityCap;
  }
  if (
    record.startShortSideCap !== undefined &&
    Number.isInteger(record.startShortSideCap) &&
    record.startShortSideCap >= 1 &&
    record.startShortSideCap <= 8_191
  ) {
    payload.start_short_side_cap = record.startShortSideCap;
  }
  return JSON.stringify(payload);
}

/**
 * 종류별 기록 개수를 사람이 을 수 있는 한 줄 요약으로 만든다. 설정 UI의
 * 내보내기 확인 문구에 쓰인다(§16 "공유 전 사용자 확인"). 종류 이름은
 * 소문자·밑줄 16자 이하만 인정하고 나머지는 버린다.
 */
export function summarizeCounts(counts: Readonly<Record<string, number>>): string {
  return Object.keys(counts)
    .filter(
      (key) =>
        /^[a-z_]{1,16}$/.test(key) &&
        Number.isInteger(counts[key]) &&
        counts[key] >= 0,
    )
    .sort()
    .map((key) => `${key} ${counts[key]}`)
    .join(", ");
}
