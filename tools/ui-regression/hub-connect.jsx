import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import Hub from '../../apps/viewer-expo/app/index';
import Host from '../../apps/viewer-expo/app/host';
import { LanguageProvider } from '../../apps/viewer-expo/src/i18n';
import * as session from '../../apps/viewer-expo/src/session';
import * as gate from '../../apps/viewer-expo/src/auto-reconnect';
import * as pairing from '../../apps/viewer-expo/src/pairing';
import { ControlRequestError } from '../../apps/viewer-expo/src/control';
import { transport } from './host-control-io';
import { hostIo } from './host-io';
import { refocus } from './hub-connect-io';
import { io } from './viewer-io';
Object.assign(window, { session, gate, pairing, transport, hostIo, refocus, viewerIo:io, ControlRequestError });
function Fixture() {
  const [screen, showScreen] = useState(null);
  window.showScreen = showScreen;
  return <LanguageProvider>{screen === 'hub' ? <Hub /> : screen === 'host' ? <Host /> : null}</LanguageProvider>;
}
createRoot(document.getElementById('root')).render(<Fixture />);
