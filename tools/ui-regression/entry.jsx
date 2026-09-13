import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import HostApp from "../../apps/host-desktop/src/App";
import Modal from "../../apps/host-desktop/src/Modal";
import {
  usePrivacySettings,
  useClipboardShare,
} from "../../apps/host-desktop/src/Privacy";

// Control only the Tauri transport; all hook and dialog behavior is production.
const pending = new Map();
Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke(command) {
      return new Promise((resolve, reject) => {
        pending.set(command, [
          ...(pending.get(command) ?? []),
          { resolve, reject },
        ]);
      });
    },
  },
  settle(command, value, fail = false) {
    const operation = pending.get(command)?.shift();
    if (!operation) throw new Error(`No pending ${command}`);
    if (fail) operation.reject(value);
    else operation.resolve(value);
  },
});
function SettingsFixture() {
  const privacy = usePrivacySettings();
  const clipboard = useClipboardShare();
  Object.assign(window, { settings: { ...privacy, ...clipboard } });
  return (
    <section>
      <button id="lock" onClick={privacy.toggleLockOnDisconnect}>
        {String(privacy.lockOnDisconnect)}
      </button>
      <output id="curtain">{String(privacy.privacyCurtain)}</output>
      <output id="settings-state">
        {JSON.stringify({ ...privacy, ...clipboard })}
      </output>
    </section>
  );
}
function App() {
  const [parent, setParent] = useState(false),
    [child, setChild] = useState(false);
  return (
    <main>
      <SettingsFixture />
      <button id="open-parent" onClick={() => setParent(true)}>
        Open parent
      </button>
      {parent && (
        <Modal ariaLabel="Parent" onClose={() => setParent(false)}>
          <button id="open-child" onClick={() => setChild(true)}>
            Open child
          </button>
          <button id="close-parent" onClick={() => setParent(false)}>
            Close parent
          </button>
          {child && (
            <Modal ariaLabel="Child" onClose={() => setChild(false)}>
              <button id="child-first">First</button>
              <button id="child-last">Last</button>
            </Modal>
          )}
        </Modal>
      )}
    </main>
  );
}
localStorage.setItem("leftcar_lang", "en");
createRoot(document.getElementById("root")).render(
  location.search.includes("dashboard") ? <HostApp /> : <App />,
);
