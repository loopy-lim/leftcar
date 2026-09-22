import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert } from "react-native";
import { useQuery } from "@tanstack/react-query";
import type { CatalogView, DisplayInfo, ExtendedDisplayMode, ExtendedDisplayPosition, ExtendedDisplayStatus } from "./control";
import { captureRequestContext } from "./session";
import { requestForCurrentSelection } from "./catalog-helpers";
import { currentTranslation } from "./language-store";
import { LocalizedError } from "./localized-error";
import { readViewerDisplayMetrics, type StreamLauncher } from "./launch-stream";
import { extensionDisplayFor, openExtendedDisplay } from "./extension-display";
import type { ActiveStream } from "./catalog-model-types";

type Operation = "open" | "remove" | "resize" | "arrange";
export function useExtensionDisplay({ host, catalog, launcher, streams, refetchCatalog, openDisplay, stopStream }: {
  host: string; catalog?: CatalogView; launcher?: StreamLauncher; streams: ActiveStream[];
  refetchCatalog: () => Promise<{ data?: CatalogView }>;
  openDisplay: (display: DisplayInfo) => Promise<unknown>;
  stopStream: (stream: ActiveStream) => Promise<unknown>;
}) {
  const lifetime = useMemo(() => ({ active: true, busy: false }), [host]);
  useEffect(() => { lifetime.active = true; return () => { lifetime.active = false; }; }, [lifetime]);
  const [activity, setActivity] = useState<{ owner: typeof lifetime; operation: Operation | null; error: string | null }>();
  const operation = activity?.owner === lifetime ? activity.operation : null;
  const error = activity?.owner === lifetime ? activity.error : null;
  const statusQuery = useQuery({
    queryKey: ["extended-display", host],
    queryFn: () => requestForCurrentSelection(host)<ExtendedDisplayStatus>("getVirtualDisplay"),
    enabled: catalog?.virtualDisplayControl === true,
    refetchInterval: 3000,
    retry: false,
  });
  const { refetch: refetchStatus } = statusQuery;
  const status = statusQuery.data;
  const extensionRemovalPending = status?.removalPending ?? Boolean(catalog?.virtualDisplayPendingRemoval);
  const extensionDisplay = extensionDisplayFor(status && catalog ? {
    ...catalog,
    virtualDisplayPendingRemoval: extensionRemovalPending ? catalog.virtualDisplayPendingRemoval ?? "pending" : undefined,
    virtualDisplaySourceId: status.live?.sourceId,
  } : catalog);
  // Refresh the source list once for each observed identity/removal transition.
  useEffect(() => {
    if (status) void refetchCatalog();
  }, [status?.live?.sourceId, status?.removalPending, refetchCatalog]);
  const supported = catalog?.virtualDisplayControl ?? catalog?.platform === "macos";
  const run = useCallback(async (kind: Operation, work: (request: ReturnType<typeof requestForCurrentSelection>, current: () => boolean) => Promise<void>) => {
    if (lifetime.busy || !lifetime.active) return;
    const origin = captureRequestContext();
    const current = () => lifetime.active && origin !== null && captureRequestContext()?.selectionGeneration === origin.selectionGeneration;
    if (!current()) return;
    lifetime.busy = true;
    setActivity({ owner: lifetime, operation: kind, error: null });
    try { await work(requestForCurrentSelection(host), current); }
    catch (cause) {
      if (current()) {
        const text = String(cause);
        const message = cause instanceof LocalizedError ? cause.format() : currentTranslation().viewer[
          text.includes("in use") ? "extInUse" : text.includes("source_access") ? "errSourceAccess"
            : text.includes("removal pending") ? "extRemovingLabel" : "extActionFailed"];
        setActivity({ owner: lifetime, operation: null, error: message });
      }
    } finally {
      lifetime.busy = false;
      if (current()) {
        setActivity(previous => previous?.owner === lifetime ? { ...previous, operation: null } : previous);
        await Promise.allSettled([refetchCatalog(), ...(catalog?.virtualDisplayControl ? [refetchStatus()] : [])]);
      }
    }
  }, [host, lifetime, refetchCatalog, refetchStatus, catalog?.virtualDisplayControl]);
  const refresh = useCallback(async () => (await refetchCatalog()).data?.displays ?? [], [refetchCatalog]);
  const openExtension = useCallback((mode?: ExtendedDisplayMode) => run("open", async (request, current) => {
    if (extensionRemovalPending) return;
    const requested = mode ?? (status?.suggested?.source === "pendingResize" ? status.suggested : undefined);
    await openExtendedDisplay({ existing: extensionDisplay, isCurrent: current, refresh, open: openDisplay,
      create: async () => {
        const metrics = await readViewerDisplayMetrics(launcher);
        if (!current()) throw new Error("cancelled");
        return request<{ sourceId: string }>("createVirtualDisplay", { ...metrics, ...(requested ? { mode: { width: requested.width, height: requested.height, scale: requested.scale } } : {}) });
      },
    });
  }), [run, extensionDisplay, extensionRemovalPending, refresh, openDisplay, launcher, status?.suggested]);
  const confirm = (resize: boolean) => new Promise<boolean>(resolve => {
    const t = currentTranslation();
    Alert.alert(resize ? t.viewer.extResizeTitle : t.viewer.extRemoveConfirmTitle,
      resize ? t.viewer.extResizeConfirm : t.viewer.extRemoveConfirmDesc,
      [{ text: t.common.cancel, style: "cancel", onPress: () => resolve(false) },
        { text: resize ? t.viewer.extApplySize : t.viewer.extRemoveButton, style: resize ? "default" : "destructive", onPress: () => resolve(true) }],
      { cancelable: true, onDismiss: () => resolve(false) });
  });
  const removeExtension = () => run("remove", async (request, current) => {
    if (!await confirm(false) || !current()) return;
    await request("removeVirtualDisplay", {});
  });
  const resizeExtension = (mode: ExtendedDisplayMode) => run("resize", async (request, current) => {
    if (!await confirm(true) || !current()) return;
    const affected = streams.filter(stream => stream.sourceId === extensionDisplay?.sourceId);
    await Promise.all(affected.map(stream => current() ? stopStream(stream) : Promise.resolve()));
    if (!current()) return;
    await openExtendedDisplay({ isCurrent: current, refresh, open: openDisplay,
      create: () => request<{ sourceId: string }>("resizeVirtualDisplay", mode),
    });
  });
  const arrangeExtension = (position: ExtendedDisplayPosition) => run("arrange", async request => {
    await request("arrangeVirtualDisplay", { position });
  });
  return { extensionDisplay, extensionRemovalPending, extensionSupported: supported,
    extensionStatus: statusQuery.data, extensionConfigurable: catalog?.virtualDisplayControl === true,
    extensionOperation: operation, extensionError: error,
    handleCreateExtensionDisplay: openExtension, handleRemoveExtensionDisplay: removeExtension,
    handleResizeExtensionDisplay: resizeExtension, handleArrangeExtensionDisplay: arrangeExtension };
}
