import React, { useCallback, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useExtensionDisplay } from '../../apps/viewer-expo/src/use-extension-display';
import { connectHost } from '../../apps/viewer-expo/src/session';
import { extensionIo } from './extension-control-io';
const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
const staleCatalog = { platform: 'macos', captureBackends: [], displays: [], virtualDisplayControl: true, virtualDisplayPendingRemoval: 'display:removed' };
const opened = [];
window.extensionIo = extensionIo;
window.opened = opened;
function Fixture() {
  const [host, setHost] = useState('192.168.0.42:9123');
  const refetchCatalog = useCallback(async () => ({ data: staleCatalog }), []);
  const model = useExtensionDisplay({ host, catalog: staleCatalog, streams: [], refetchCatalog, openDisplay: async d => opened.push(d), stopStream: async () => {} });
  window.extensionModel = model;
  window.selectExtensionHost = async target => { await connectHost(target, 9123); setHost(`${target}:9123`); };
  return <output>{host}|{String(model.extensionRemovalPending)}|{model.extensionOperation ?? 'idle'}|{model.extensionError ?? 'ok'}</output>;
}
connectHost('192.168.0.42', 9123).then(() => createRoot(document.getElementById('root')).render(<QueryClientProvider client={queryClient}><Fixture /></QueryClientProvider>));
