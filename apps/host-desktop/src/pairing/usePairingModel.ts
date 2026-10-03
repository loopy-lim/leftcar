import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import {
  getTranslation,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import {
  getPairedDeviceState,
  confirmSourceGrants,
  sourceGrantUncertaintyEpoch,
  markSourceGrantsUncertain,
  receivePairedDeviceState,
  receiveRevokeOutcome,
  subscribePairedDeviceState,
  type PairedDevice,
  type PairedDeviceState,
  type RevokeOutcome,
  type SourceGrantView,
} from "../paired-device-state";
import { formatHostAddress } from "../hostState";
import type { RevokeConfirm } from "../RevokeConfirmDialog";
import type { ActivePairingSession } from "./PairingQrCard";
import type { PendingPairingView } from "./PendingApprovalSection";
interface PairingSessionView {
  offer_id: string;
  qr_payload: string;
  code: string;
  expires_in_secs: number;
}
function connectionErrorMessage(cause: unknown, t: TranslationSchema): string {
  const message = String(
    cause instanceof Error ? cause.message : cause,
  ).toLowerCase();
  if (message.includes("no lan interface")) return t.host.networkNotFoundError;
  if (message.includes("persistence")) return t.host.appServiceInitError;
  return t.host.connectionCheckError;
}
export function usePairingModel(language: SupportedLanguage) {
  const t = getTranslation(language);

  const [session, setSession] = useState<ActivePairingSession | null>(null);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const pairingBusy = useRef(false);
  const mounted = useRef(false);
  const ownedOffer = useRef<string | null>(null);
  const revokeBusy = useRef(false);
  const decisionBusy = useRef(false);
  const grantsBusy = useRef(false);
  const pairedState = useSyncExternalStore(
    subscribePairedDeviceState,
    getPairedDeviceState,
  );
  const devices = pairedState.devices;
  const [pendingRequests, setPendingRequests] = useState<PendingPairingView[]>(
    [],
  );
  const [decidingId, setDecidingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revokeError, setRevokeError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [devicesReady, setDevicesReady] = useState(
    () => getPairedDeviceState().revision >= 0,
  );
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const [pendingError, setPendingError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [approvingScreens, setApprovingScreens] = useState<string | null>(null);
  const [revokeConfirm, setRevokeConfirm] = useState<RevokeConfirm | null>(
    null,
  );
  const [copiedCode, setCopiedCode] = useState(false);
  const [copiedIp, setCopiedIp] = useState(false);
  const [lanIp, setLanIp] = useState<string | null>(null);
  const [controlPort, setControlPort] = useState(7777);
  const [now, setNow] = useState(Date.now());
  const deviceCountRef = useRef(devices.length);

  useEffect(() => {
    void invoke<string | null>("get_lan_ip")
      .then(setLanIp)
      .catch(() => {});
    void invoke<number>("get_control_port")
      .then(setControlPort)
      .catch(() => {});
  }, []);

  useEffect(() => {
    deviceCountRef.current = devices.length;
  }, [devices.length]);

  const refreshDevices = useCallback(async () => {
    try {
      const snapshot = await invoke<PairedDeviceState>(
        "list_paired_device_state",
      );
      if (snapshot.devices.length > deviceCountRef.current) {
        setSession(null);
        setSuccess(t.host.pairingSucceeded);
      }
      setDevicesReady(true);
      setDevicesError(null);
      receivePairedDeviceState(snapshot);
    } catch {
      setDevicesError(t.host.deviceListError);
    }
  }, [t]);

  const startPairing = useCallback(async () => {
    if (pairingBusy.current) return;
    pairingBusy.current = true;
    setStarting(true);
    setCopiedCode(false);
    setError(null);
    setSuccess(null);
    try {
      const view = await invoke<PairingSessionView>("begin_pairing");
      if (!view.offer_id) throw new Error("Pairing offer identity missing");
      ownedOffer.current = view.offer_id;
      if (!mounted.current) {
        await invoke("cancel_pairing", { offerId: view.offer_id });
        ownedOffer.current = null;
        return;
      }
      setSession(null);
      const qrDataUrl = await QRCode.toDataURL(view.qr_payload, {
        width: 220,
        margin: 1,
        color: {
          dark: "#09090b",
          light: "#ffffff",
        },
      });
      if (!mounted.current) {
        await invoke("cancel_pairing", { offerId: view.offer_id });
        return;
      }
      setSession({
        offerId: view.offer_id,
        durationMs: view.expires_in_secs * 1000,
        qrDataUrl,
        code: view.code,
        expiresAt: Date.now() + view.expires_in_secs * 1000,
      });
      refreshDevices();
    } catch (e) {
      const offerId = ownedOffer.current;
      if (offerId) void invoke("cancel_pairing", { offerId }).catch(() => {});
      ownedOffer.current = null;
      if (mounted.current) setError(connectionErrorMessage(e, t));
    } finally {
      pairingBusy.current = false;
      if (mounted.current) setStarting(false);
    }
  }, [refreshDevices, t]);

  const cancelPairing = useCallback(async () => {
    const offerId = ownedOffer.current;
    if (pairingBusy.current || !offerId) return;
    pairingBusy.current = true;
    setCancelling(true);
    setError(null);
    try {
      await invoke("cancel_pairing", { offerId });
      if (ownedOffer.current === offerId) ownedOffer.current = null;
      if (mounted.current) setSession(null);
    } catch {
      if (mounted.current) setError(t.host.pairingCancelFailed);
    } finally {
      pairingBusy.current = false;
      if (mounted.current) setCancelling(false);
    }
  }, [t]);

  const copyCode = useCallback(() => {
    if (!session) return;
    navigator.clipboard.writeText(session.code).then(
      () => {
        setCopiedCode(true);
        setTimeout(() => setCopiedCode(false), 2000);
      },
      () => setError(t.host.copyFailed),
    );
  }, [session, t]);

  const copyIp = useCallback(() => {
    if (!lanIp) return;
    navigator.clipboard.writeText(formatHostAddress(lanIp, controlPort)).then(
      () => {
        setCopiedIp(true);
        setTimeout(() => setCopiedIp(false), 2000);
      },
      () => setError(t.host.copyFailed),
    );
  }, [lanIp, controlPort, t]);

  const revoke = useCallback(
    async (deviceId: string) => {
      if (revokeBusy.current) return;
      revokeBusy.current = true;
      setRevoking(deviceId);
      setRevokeError(null);
      try {
        receiveRevokeOutcome(
          await invoke<RevokeOutcome>("revoke_paired_device", { deviceId }),
        );
        await refreshDevices();
        setRevokeConfirm(null);
        setSuccess(t.host.revokeDone);
      } catch {
        setRevokeError(t.host.revokeFailed);
      } finally {
        revokeBusy.current = false;
        setRevoking(null);
      }
    },
    [refreshDevices, t],
  );

  const revokeAll = useCallback(async () => {
    if (revokeBusy.current) return;
    revokeBusy.current = true;
    setRevoking("all");
    setRevokeError(null);
    try {
      receiveRevokeOutcome(await invoke<RevokeOutcome>("revoke_all_devices"));
      await refreshDevices();
      setRevokeConfirm(null);
      setSuccess(t.host.revokeDone);
    } catch {
      setRevokeError(t.host.revokeFailed);
    } finally {
      revokeBusy.current = false;
      setRevoking(null);
    }
  }, [refreshDevices, t]);

  // 승인 때 화면 목록이 비어 있었던 기기(화면기록 권한 전 페어링 등)를 위한
  // 수동 화면 허용 — "승인 대기"가 뜬 그 자리에서 결정한다.
  const approveScreens = useCallback(
    async (device: PairedDevice) => {
      if (grantsBusy.current) return;
      const startedAtEpoch = sourceGrantUncertaintyEpoch(
        device.source_grants.credentialId,
      );
      grantsBusy.current = true;
      setApprovingScreens(device.device_id);
      setError(null);
      try {
        const displays =
          await invoke<{ sourceId: string | null }[]>("list_host_sources");
        const sourceIds = displays
          .map((display) => display.sourceId)
          .filter((sourceId): sourceId is string => sourceId !== null);
        const grants = await invoke<SourceGrantView>("set_source_grants", {
          deviceId: device.device_id,
          sourceIds,
          credentialId: device.source_grants.credentialId,
        });
        confirmSourceGrants(device.device_id, grants, startedAtEpoch);
        await refreshDevices();
        setSuccess(t.host.screensApproved);
      } catch (e) {
        markSourceGrantsUncertain(
          device.device_id,
          device.source_grants.credentialId,
          String(e instanceof Error ? e.message : e),
        );
        setError(connectionErrorMessage(e, t));
      } finally {
        grantsBusy.current = false;
        setApprovingScreens(null);
      }
    },
    [refreshDevices, t],
  );

  const refreshPending = useCallback(async () => {
    try {
      setPendingRequests(
        await invoke<PendingPairingView[]>("list_pending_pairings"),
      );
      setPendingError(null);
    } catch {
      setPendingError(t.host.pendingListError);
    }
  }, [t]);

  const decideRequest = useCallback(
    async (
      command: "approve_pending_pairing" | "reject_pending_pairing",
      offerId: string,
    ) => {
      if (decisionBusy.current) return;
      decisionBusy.current = true;
      setError(null);
      setDecidingId(offerId);
      try {
        await invoke(command, { offerId });
        setPendingRequests((current) =>
          current.filter((request) => request.offer_id !== offerId),
        );
      } catch (e) {
        setError(connectionErrorMessage(e, t));
      } finally {
        decisionBusy.current = false;
        setDecidingId(null);
      }
    },
    [t],
  );

  const approveRequest = useCallback(
    (offerId: string) => decideRequest("approve_pending_pairing", offerId),
    [decideRequest],
  );

  const denyRequest = useCallback(
    (offerId: string) => decideRequest("reject_pending_pairing", offerId),
    [decideRequest],
  );

  useEffect(() => {
    refreshDevices();
    const interval = setInterval(refreshDevices, 2000);
    return () => clearInterval(interval);
  }, [refreshDevices]);

  useEffect(() => {
    refreshPending();
    const interval = setInterval(refreshPending, 1500);
    return () => clearInterval(interval);
  }, [refreshPending]);

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      const offerId = ownedOffer.current;
      ownedOffer.current = null;
      if (offerId) void invoke("cancel_pairing", { offerId }).catch(() => {});
    };
  }, []);

  const expired = session !== null && now >= session.expiresAt;
  const remainingMs = session ? Math.max(0, session.expiresAt - now) : 0;
  const progressPercent = session
    ? Math.max(0, Math.min(100, (remainingMs / session.durationMs) * 100))
    : 0;

  useEffect(() => {
    if (session && expired) {
      invoke("cancel_pairing", { offerId: session.offerId }).catch(() => {});
    }
  }, [session, expired]);

  return {
    session,
    starting,
    cancelling,
    devices,
    pendingRequests,
    decidingId,
    error,
    pairedState,
    revokeError,
    success,
    devicesReady,
    devicesError,
    pendingError,
    revoking,
    approvingScreens,
    revokeConfirm,
    copiedCode,
    copiedIp,
    lanIp,
    controlPort,
    expired,
    remainingMs,
    progressPercent,
    setRevokeConfirm,
    setRevokeError,
    revoke,
    revokeAll,
    refreshDevices,
    refreshPending,
    approveRequest,
    denyRequest,
    startPairing,
    cancelPairing,
    copyCode,
    copyIp,
    approveScreens,
  };
}
