import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import Catalog from '../../apps/viewer-expo/app/catalog';
import { LanguageProvider } from '../../apps/viewer-expo/src/i18n';
import * as session from '../../apps/viewer-expo/src/session';
import { setRandomSource } from '../../apps/viewer-expo/src/secure-channel';
import { io } from './viewer-io.js';
setRandomSource(n => new Uint8Array(n).fill(7));
globalThis.__leftcarUsbRuntime = {
  getUsbNative: () => ({ getAccessoryState: async () => ({ attached: true, controlPort: 7777 }) }),
  subscribeUsbNative: () => ({ remove() {} }),
};
const fileIo = { picks: [], discarded: [], finalized: [] };
globalThis.__leftcarFileIo = {
  pickSendFile: () => new Promise((resolve, reject) => fileIo.picks.push({ resolve, reject })),
  createReceivedSink: async name => ({ path: `/fixture/${name}`, appendBase64: async () => {}, finalize: async () => fileIo.finalized.push(name), discard: async () => fileIo.discarded.push(name) }),
};
Object.assign(window, { viewerIo: io, session, fileIo });
const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
function Fixture() {
  const [mounted, setMounted] = useState(true);
  window.mountCatalogScreen = setMounted;
  return <LanguageProvider><QueryClientProvider client={client}>{mounted && <Catalog />}</QueryClientProvider></LanguageProvider>;
}
session.connectHost('192.168.0.42').then(() => createRoot(document.getElementById('root')).render(<Fixture />));
