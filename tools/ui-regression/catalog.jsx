import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import * as session from "../../apps/viewer-expo/src/session";
import { streamSessionStore } from "../../apps/viewer-expo/src/stream-session-store";
import { connectHost } from "../../apps/viewer-expo/src/session";
import { useStreamController } from "../../apps/viewer-expo/src/use-stream-controller";
import { useCatalogModel } from "../../apps/viewer-expo/src/use-catalog-model";
import { setRandomSource } from "../../apps/viewer-expo/src/secure-channel";
import { io } from "./viewer-io";
import { ReservedStream } from "../../apps/viewer-expo/src/reserved-stream";
import { DecoderReservations } from "../../apps/viewer-expo/src/decoder-budget";
import { AmbiguousControlError, ControlRequestError } from "../../apps/viewer-expo/src/control-error";
setRandomSource((n) => new Uint8Array(n).fill(7));
globalThis.__leftcarUsbRuntime = {
  getUsbNative: () => ({
    getAccessoryState: async () => ({ attached: true, controlPort: 7777 }),
  }),
  subscribeUsbNative: (listener) => {
    io.usbSubscribers ??= new Set();
    io.usbSubscribers.add(listener);
    return { remove() { io.usbSubscribers.delete(listener); } };
  },
};
Object.assign(window, { viewerIo: io, session, streamSessionStore });
function Catalog() {
  const model = useCatalogModel();
  Object.assign(window, { model });
  return <output>{model.streams.length}</output>;
}
const closeRequests = [];
async function createFixtureReservation(active) {
  const demand = { split: false, target: active.activeTarget };
  const control = session.controlClient();
  const reservation = new ReservedStream(active.port, demand, {
    async prepareStream() {}, async openStream() { return `src-${active.port}`; },
    async cancelPreparedStream() {}, async getStreamGeneration() { return 'fixture-native-owner'; },
    closeStream: () => new Promise(resolve => closeRequests.push({ resolve })),
  }, control.request.bind(control), new DecoderReservations({ maxInstances: 2 }));
  await reservation.run(demand, async () => active);
  return reservation;
}
const adaptiveRequests = [];
const restoreRequests = [];
const restoreDeferred = (active) => new Promise((resolve, reject) => {
  restoreRequests.push({ active, resolve, reject });
});
const reconfigureDeferred = (active, target, qualityState) => new Promise((resolve, reject) => {
  adaptiveRequests.push({ active, target, qualityState, resolve, reject });
});
function AdaptiveController() {
  const controller = useStreamController(restoreDeferred, reconfigureDeferred);
  Object.assign(window, { adaptiveController: controller });
  return <output>{controller.streams.length}</output>;
}
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});
Object.assign(window, { queryClient, adaptiveRequests, restoreRequests, closeRequests, createFixtureReservation, AmbiguousControlError, ControlRequestError });
function Fixture() {
  const [mounted, setMounted] = useState(true);
  Object.assign(window, { mountCatalog: setMounted });
  return (
    <QueryClientProvider client={queryClient}>
      {mounted && (location.search.includes("adaptive") ? <AdaptiveController /> : <Catalog />)}
    </QueryClientProvider>
  );
}
connectHost("192.168.0.42").then(() =>
  createRoot(document.getElementById("root")).render(<Fixture />),
);
