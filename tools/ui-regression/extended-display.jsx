import React from "react";
import { createRoot } from "react-dom/client";
import ExtendedDisplayCard from "../../apps/host-desktop/src/components/ExtendedDisplayCard";
import { translations } from "@leftcar/ui-tokens";

const status = {
  supported: true,
  suggested: { width: 1280, height: 720, scale: 2, source: "viewerMetrics" },
  removalPending: location.search.includes("pending"),
};
window.displayIo = { status, calls: [] };
window.__TAURI_INTERNALS__ = {
  async invoke(command, args) {
    window.displayIo.calls.push({ command, args });
    return structuredClone(window.displayIo.status);
  },
};
createRoot(document.getElementById("root")).render(<ExtendedDisplayCard t={translations.en} />);
