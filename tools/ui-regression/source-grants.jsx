import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import SourceGrantEditor from "../../apps/host-desktop/src/SourceGrantEditor";
const io = { refreshFails: false, calls: [], pending: [], grants: { credentialId: "synthetic-credential", stateRevision: 0, sourceIds: [], revision: 0, reviewRequired: true }, sources: [
  { sourceId: "macos:display:a", index: 2, name: "Synthetic A", width: 1920, height: 1080 },
  { sourceId: "macos:display:b", index: 0, name: "Synthetic B", width: 1280, height: 720 },
] };
window.grantIo = io;
window.__TAURI_INTERNALS__ = { invoke(command, args) {
  io.calls.push({ command, args });
  if (command === "list_host_sources") return Promise.resolve(io.sources);
  if (command === "set_source_grants") return new Promise((resolve, reject) => io.pending.push({ args, resolve, reject }));
  return Promise.reject(new Error(`unexpected command ${command}`));
} };
function Fixture() {
  const [grants, setGrants] = useState(io.grants);
  return <SourceGrantEditor deviceId="synthetic-device" grants={grants} language="ko" onSaved={async (_device, confirmed) => { setGrants(confirmed); }} />;
}
createRoot(document.getElementById("root")).render(<Fixture />);
