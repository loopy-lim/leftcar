/** Only closed codes cross the presentation boundary; never retain the raw cause. */
export type KmIssue = "consentRequired" | "shizukuUnavailable" | "shizukuPermission" | "accessibilityPermission" | "unsupported" | "displayUnavailable" | "startFailed" | "stopFailed" | "busy"
  | "connectionRequired" | "wifiRequired" | "pairingRequired" | "keyBridgeUnsupported" | "helperUnavailable" | "observerConflict" | "nativeTimeout" | "nativeConfigInvalid"
  | "nativeStopped" | "hostInputPermission" | "hostConsentRequired" | "hostUnsafe" | "permissionsLost" | "displayChanged" | "mouseCaptureLost" | "hostStopped";
export type KmStartStage = "connectionSetup" | "androidPreparation" | "hostApproval" | "nativeHandshake";
export interface KmFailure { issue: KmIssue; stage?: KmStartStage }
export class KmError extends Error implements KmFailure {
  constructor(readonly issue: KmIssue, readonly stage?: KmStartStage) { super(issue); this.name = "KmError"; }
}

// Exact matching is intentional: a known prefix plus arbitrary data stays generic.
const knownReasons = new Map<string, KmIssue>([
  ["Leftcar 접근성 권한이 필요합니다", "accessibilityPermission"],
  ["USB로 Shizuku를 ADB 모드에서 시작하세요", "shizukuUnavailable"],
  ["Shizuku 사용 권한이 필요합니다", "shizukuPermission"],
  ["km_shizuku_unavailable", "shizukuUnavailable"],
  ["KeyBridge 입력 공유 연동 API가 필요합니다. KeyBridge를 활성화한 채 지원 업데이트를 적용하세요", "keyBridgeUnsupported"],
  ["KeyBridge physical key ownership unavailable", "keyBridgeUnsupported"],
  ["물리 키 장치가 연결되어 KeyBridge 입력 소유권을 다시 확인해야 합니다", "keyBridgeUnsupported"],
  ["KeyBridge 입력 소유권 확인 전 물리 키 전달을 중지했습니다", "keyBridgeUnsupported"],
  ["KeyBridge 입력 소유권 연결이 종료되었습니다", "keyBridgeUnsupported"],
  ["UHID 또는 Android 16 마우스 관찰 기능을 사용할 수 없습니다", "unsupported"],
  ["다른 접근성 서비스가 마우스를 관찰 중입니다", "observerConflict"],
  ["다른 접근성 서비스가 키 입력을 필터링 중입니다", "observerConflict"],
  ["Shizuku 입력 서비스 연결 시간이 초과되었습니다", "helperUnavailable"],
  ["Shizuku 입력 서비스가 종료되었습니다", "helperUnavailable"],
  ["Shizuku 연결이 종료되었습니다", "helperUnavailable"],
  ["Shizuku 입력 서비스가 변경되었습니다", "helperUnavailable"],
  ["Shizuku 입력 서비스를 연결할 수 없습니다", "helperUnavailable"],
  ["Shizuku 입력 서비스를 시작하지 못했습니다", "helperUnavailable"],
  ["백그라운드 입력 서비스를 시작하지 못했습니다", "helperUnavailable"],
  ["Shizuku가 종료되었습니다", "shizukuUnavailable"],
  ["입력 공유 상태 확인 시간이 초과되었습니다", "nativeTimeout"],
  ["km_invalid_config", "nativeConfigInvalid"],
  ["Enable Input Monitoring for Leftcar Host in macOS System Settings", "hostInputPermission"],
  ["Enable Accessibility for Leftcar Host in macOS System Settings", "hostInputPermission"],
  ["Approve both input-sharing directions for this peer on the Mac", "hostConsentRequired"],
  ["Approve both directions on the tablet before sharing input", "consentRequired"],
  ["Unlock the Mac and leave secure password input before sharing", "hostUnsafe"],
  ["접근성 서비스가 중단되었습니다", "accessibilityPermission"],
  ["접근성 권한 또는 서비스가 종료되었습니다", "accessibilityPermission"],
  ["입력 공유 응답 또는 권한이 만료되었습니다", "permissionsLost"],
  ["화면 배치가 변경되었습니다. 입력 공유를 다시 시작하세요", "displayChanged"],
  ["물리 마우스 캡처가 종료되었습니다", "mouseCaptureLost"],
  ["Mac에서 입력 공유를 중지했습니다", "hostStopped"],
]);
export function kmReasonIssue(cause: unknown): KmIssue | null {
  if (typeof cause === "string") return knownReasons.get(cause) ?? null;
  if (!cause || typeof cause !== "object") return null;
  try {
    for (const field of ["code", "reason", "message"] as const) {
      const value = (cause as Record<string, unknown>)[field];
      if (typeof value === "string") {
        const issue = knownReasons.get(value);
        if (issue) return issue;
      }
    }
  } catch { return null; }
  return null;
}
export function kmStartFailure(cause: unknown, stage: KmStartStage): KmError {
  return new KmError(cause instanceof KmError ? cause.issue : kmReasonIssue(cause) ?? "startFailed", stage);
}
