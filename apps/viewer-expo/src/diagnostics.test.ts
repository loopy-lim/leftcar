import { describe, expect, it } from "vitest";
import {
  BACKUP_GENERATIONS,
  BOOT_DETECTION_UPTIME_MS,
  MAX_FILE_BYTES,
  MAX_RECORD_BYTES,
  SAMPLE_INTERVAL_MS,
  formatSession,
  summarizeCounts,
  type DiagnosticsSessionRecord,
} from "./diagnostics";

describe("diagnostics 상수", () => {
  // §16 명세 수치를 상수로 고정한다 — 스토어·샘플러(Kotlin)와 같은 값을
  // 양쪽 테스트가 검증해 경계에서 드리프트를 막는다.
  it("pins the §16 storage and sampling numbers", () => {
    expect(SAMPLE_INTERVAL_MS).toBe(15_000);
    expect(MAX_RECORD_BYTES).toBe(1024);
    expect(MAX_FILE_BYTES).toBe(5 * 1024 * 1024);
    expect(BACKUP_GENERATIONS).toBe(1);
    expect(BOOT_DETECTION_UPTIME_MS).toBe(120_000);
  });
});

describe("formatSession", () => {
  const base: DiagnosticsSessionRecord = { ts: 1_760_000_000_000, upMs: 1_234_567, event: "session_start" };

  it("formats the snake_case session record with fixed key order", () => {
    expect(formatSession(base)).toBe(
      '{"ts":1760000000000,"up_ms":1234567,"kind":"session","event":"session_start"}',
    );
  });

  it("includes optional fields in schema order when present", () => {
    expect(
      formatSession({
        ...base,
        appVersion: "0.1.10",
        osApi: 36,
        bootDetected: true,
        profile: "splitVertical",
        decoderSlots: 2,
      }),
    ).toBe(
      '{"ts":1760000000000,"up_ms":1234567,"kind":"session","event":"session_start","app_version":"0.1.10","os_api":36,"boot_detected":true,"profile":"splitVertical","decoder_slots":2}',
    );
  });

  it("omits optional fields that are undefined", () => {
    const line = formatSession({ ...base, event: "stream_stop" });
    expect(line).toBe('{"ts":1760000000000,"up_ms":1234567,"kind":"session","event":"stream_stop"}');
  });

  it("rounds non-integer clock values instead of emitting floats", () => {
    expect(formatSession({ ...base, ts: 1.6, upMs: 1_234.9 })).toBe(
      '{"ts":2,"up_ms":1235,"kind":"session","event":"session_start"}',
    );
  });

  it("rejects non-finite clock values as call-site bugs", () => {
    expect(() => formatSession({ ...base, ts: Number.NaN })).toThrow();
    expect(() => formatSession({ ...base, upMs: Number.POSITIVE_INFINITY })).toThrow();
  });

  it("rejects unknown event kinds", () => {
    // 이벤트 종류는 호출 지점 리터럴이다 — 목록 밖 값은 데이터 치환이 아니라
    // 프로그래머 오류다.
    expect(() =>
      formatSession({ ...base, event: "sensor_flood" as DiagnosticsSessionRecord["event"] }),
    ).toThrow();
  });

  it("caps records at the §16 limit of 1024 bytes", () => {
    const line = formatSession({
      ...base,
      appVersion: "0.1.10",
      osApi: 36,
      bootDetected: true,
      profile: "splitVertical",
      decoderSlots: 4,
    });
    expect(new TextEncoder().encode(line).length).toBeLessThanOrEqual(MAX_RECORD_BYTES);
  });
});

describe("diagnostics_redact_title_path_token_and_ip", () => {
  // docs/07 §18 필수 테스트. 허용 목록 밖 문자열(제목·IP·토큰·파일 경로)은
  // 레코드 어디에도 남지 않아야 한다.
  it("drops titles, IPs, tokens, and file paths from string fields", () => {
    const hostile = formatSession({
      ts: 1_760_000_000_000,
      upMs: 1,
      event: "stream_start",
      profile: "splitVertical 192.168.10.14 Main Display pair_token_abcd /Users/loopy/secret.txt",
      appVersion: "0.1.10 10.0.0.1 https://example.invalid/pair",
    });
    expect(hostile).toContain('"profile":"unknown"');
    expect(hostile).toContain('"app_version":"unknown"');
    expect(hostile).not.toContain("192.168");
    expect(hostile).not.toContain("Main Display");
    expect(hostile).not.toContain("pair_token_abcd");
    expect(hostile).not.toContain("/Users/loopy");
    expect(hostile).not.toContain("example.invalid");
  });

  it("keeps well-formed identifiers untouched", () => {
    // 정상 식별자(프로필·실험 ID, 버전 문자열)는 치환하면 안 된다.
    const line = formatSession({
      ts: 1_760_000_000_000,
      upMs: 1,
      event: "stream_start",
      profile: "splitVertical",
      appVersion: "0.1.10-rc.1+build.45",
    });
    expect(line).toContain('"profile":"splitVertical"');
    expect(line).toContain('"app_version":"0.1.10-rc.1+build.45"');
  });

  it("omits decoder slots that are not positive integers", () => {
    expect(
      formatSession({ ts: 1, upMs: 1, event: "stream_start", decoderSlots: 0 }),
    ).not.toContain("decoder_slots");
    expect(
      formatSession({ ts: 1, upMs: 1, event: "stream_start", decoderSlots: 2.5 }),
    ).not.toContain("decoder_slots");
    expect(
      formatSession({ ts: 1, upMs: 1, event: "stream_start", decoderSlots: 4 }),
    ).toContain('"decoder_slots":4');
  });
});

describe("summarizeCounts", () => {
  it("formats per-kind counts in sorted key order", () => {
    expect(summarizeCounts({ session: 8, sample: 120, crash: 0 })).toBe(
      "crash 0, sample 120, session 8",
    );
  });

  it("drops keys that are not plain kind identifiers", () => {
    expect(
      summarizeCounts({ sample: 1, "viewerAddr;192.168.0.1": 2, "UPPER": 3, "way_too_long_kind_identifier": 4 }),
    ).toBe("sample 1");
  });

  it("returns an empty string when nothing was recorded", () => {
    expect(summarizeCounts({})).toBe("");
    expect(summarizeCounts({ crash: -1 })).toBe("");
  });
});

describe("thermal_cap 세션 이벤트", () => {
  const base = { ts: 1_760_000_000_000, upMs: 1_234_567 };

  it("formats the cap event with its numeric fields in schema order", () => {
    expect(
      formatSession({
        ...base,
        event: "thermal_cap",
        thermalStatus: 3,
        capacityCap: 2,
        startShortSideCap: 1_080,
      }),
    ).toBe(
      '{"ts":1760000000000,"up_ms":1234567,"kind":"session","event":"thermal_cap","thermal_status":3,"capacity_cap":2,"start_short_side_cap":1080}',
    );
  });

  it("formats the cleared event without cap fields", () => {
    expect(
      formatSession({ ...base, event: "thermal_cap_cleared", thermalStatus: 1 }),
    ).toBe(
      '{"ts":1760000000000,"up_ms":1234567,"kind":"session","event":"thermal_cap_cleared","thermal_status":1}',
    );
  });

  it("omits implausible numeric fields instead of leaking them", () => {
    const line = formatSession({
      ...base,
      event: "thermal_cap",
      thermalStatus: 99,
      capacityCap: 0,
      startShortSideCap: 2.5,
    });
    expect(line).not.toContain("thermal_status");
    expect(line).not.toContain("capacity_cap");
    expect(line).not.toContain("start_short_side_cap");
    expect(line).toContain('"event":"thermal_cap"');
  });
});
