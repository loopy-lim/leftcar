import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { colors } from "@leftcar/ui-tokens";
import { Button, Field, Text, Surface, Toggle } from "../../apps/host-desktop/src/ui/primitives";
import { Action, Label, Surface as NativeSurface } from "../../apps/viewer-expo/src/ui/primitives";

function Fixture() {
  const [busy, setBusy] = useState(false);
  const [checked, setChecked] = useState(false);
  window.designPalette = colors;
  window.designClicks = window.designClicks ?? 0;
  const act = () => { window.designClicks += 1; setBusy(true); };
  const viewer = location.search.includes("viewer");
  return viewer ? <NativeSurface variant="card">
    <Label nativeID="body">Body</Label>
    <Label nativeID="caption" variant="caption" tone="muted">Caption</Label>
    <Action label="Save" busy={busy} onPress={act} />
    <Action label="Tab" variant="secondary" accessibilityRole="tab" accessibilityState={{ selected: true }} onPress={() => {}} />
  </NativeSurface> : <Surface variant="card">
    <Text id="body">Body</Text>
    <Text id="caption" variant="caption" tone="muted">Caption</Text>
    <Button busy={busy} onClick={act}>Save</Button>
    <Field aria-label="Width" invalid />
    <Toggle aria-label="Share" checked={checked} onClick={() => setChecked(!checked)} />
  </Surface>;
}
createRoot(document.getElementById("root")).render(<Fixture />);
