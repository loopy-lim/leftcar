/** expo-camera 권한 응답 중 UI 분기에 필요한 필드만 본다. */
export interface CameraPermissionInfo {
  granted: boolean;
  canAskAgain: boolean;
}

export type CameraState = "loading" | "request" | "blocked" | "live";

/**
 * 카메라 권한 응답을 화면 분기 상태로 바꾼다. blocked는 권한이 영구 거부된
 * 상태 — 앱 내 재요청으로는 풀리지 않으므로 설정 앱으로 안내해야 한다.
 */
export function resolveCameraState(
  permission: CameraPermissionInfo | null | undefined,
): CameraState {
  if (!permission) return "loading";
  if (permission.granted) return "live";
  return permission.canAskAgain ? "request" : "blocked";
}
