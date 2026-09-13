import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import Host from '../../apps/viewer-expo/app/host';
import { LanguageProvider } from '../../apps/viewer-expo/src/i18n';
import * as session from '../../apps/viewer-expo/src/session';
import { transport } from './host-control-io';
import { hostIo } from './host-io';
import { io } from './viewer-io';
import { storageIo } from './host-storage-io';
Object.assign(window, { session, transport, hostIo, storageIo, viewerIo: io });
function Fixture() {
  const [mounted, setMounted] = useState(true);
  window.mountHost = setMounted;
  return <LanguageProvider>{mounted && <Host />}</LanguageProvider>;
}
createRoot(document.getElementById('root')).render(<Fixture />);
