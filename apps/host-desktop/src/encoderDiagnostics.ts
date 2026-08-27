import type { SessionRow } from "./sessionTypes";

export interface EncoderDiagnosticsView {
  path: string;
  identity: string;
  configuration: string;
  applied: string;
  unavailable: string;
  fallback: string | null;
}

export function encoderDiagnosticsView(session: SessionRow): EncoderDiagnosticsView {
  const mode = session.encoderMode === "ave"
    ? "AVE"
    : session.encoderMode === "rtvc"
      ? "RTVC"
      : "미확인";
  const acceleration = session.encoderHardwareAccelerated === true
    ? "하드웨어"
    : session.encoderHardwareAccelerated === false
      ? "소프트웨어"
      : "가속 확인 중";
  const unavailable = [
    session.encoderUnsupportedProperties?.length
      ? `미지원 ${session.encoderUnsupportedProperties.join(", ")}`
      : null,
    session.encoderRejectedProperties?.length
      ? `거부 ${session.encoderRejectedProperties.join(", ")}`
      : null,
  ].filter((value): value is string => value !== null).join(" · ");

  return {
    path: `${mode} · ${acceleration}`,
    identity: session.encoderID || "인코더 ID 확인 중",
    configuration: `${session.encoderPreset || "preset 확인 중"} · ${session.encoderProfile || "profile 확인 중"}`,
    applied: session.encoderAppliedProperties?.join(", ") || "없음",
    unavailable: unavailable || "없음",
    fallback: session.encoderFallbackReason ?? null,
  };
}
