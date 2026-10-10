import { invoke } from "@tauri-apps/api/core";
export interface KmDeviceConsent { macToAndroid: boolean; androidToMac: boolean }
export interface KmDeviceStatus {
  owner: "local" | "macToAndroid" | "androidToMac"; armed: boolean; epoch: number;
  capabilities: { macCapture: boolean; macInjection: boolean; android: { cursorInjection: boolean; mouseObservation: boolean; mouseCapture: boolean; keyboardCapture: boolean } };
  blockedReason?: string | null;
}
type KmInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export function saveKmDeviceConsent(deviceId: string, consent: KmDeviceConsent, request: KmInvoke = invoke): Promise<KmDeviceConsent> {
  return request<KmDeviceConsent>("set_km_consent", { deviceId, ...consent });
}
export async function loadKmDevice(deviceId: string) {
  const [consent, status] = await Promise.all([
    invoke<KmDeviceConsent>("get_km_consent", { deviceId }),
    invoke<KmDeviceStatus>("get_km_status", { deviceId }),
  ]);
  return { consent, status };
}
