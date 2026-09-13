import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { connectHost } from "../../apps/viewer-expo/src/session";
import { useCatalogModel } from "../../apps/viewer-expo/src/use-catalog-model";
import { setRandomSource } from "../../apps/viewer-expo/src/secure-channel";
import { io } from "./viewer-io";
setRandomSource((n) => new Uint8Array(n).fill(7));
globalThis.__leftcarUsbRuntime = {
  getUsbNative: () => ({
    getAccessoryState: async () => ({ attached: true, controlPort: 7777 }),
  }),
  subscribeUsbNative: () => ({ remove() {} }),
};
Object.assign(window, { viewerIo: io });
function Catalog() {
  const model = useCatalogModel();
  Object.assign(window, { model });
  return <output>{model.streams.length}</output>;
}
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});
function Fixture() {
  const [mounted, setMounted] = useState(true);
  Object.assign(window, { mountCatalog: setMounted });
  return (
    <QueryClientProvider client={queryClient}>
      {mounted && <Catalog />}
    </QueryClientProvider>
  );
}
connectHost("192.168.0.42").then(() =>
  createRoot(document.getElementById("root")).render(<Fixture />),
);
