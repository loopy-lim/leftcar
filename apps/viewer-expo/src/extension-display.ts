import type { CatalogView, DisplayInfo } from "./control";
import { isExtensionDisplay } from "./catalog-helpers";
import { LocalizedError } from "./localized-error";

export function extensionDisplayFor(catalog?: CatalogView): DisplayInfo | undefined {
  if (!catalog || catalog.virtualDisplayPendingRemoval) return undefined;
  return catalog.displays.find(display => catalog.virtualDisplayControl !== undefined || catalog.virtualDisplaySourceId
    ? display.sourceId === catalog.virtualDisplaySourceId : isExtensionDisplay(display.name));
}

/** Creation and catalog propagation stay tied to the initiating Host selection. */
export async function openExtendedDisplay(options: {
  existing?: DisplayInfo;
  isCurrent: () => boolean;
  create: () => Promise<{ sourceId: string }>;
  refresh: () => Promise<DisplayInfo[]>;
  open: (display: DisplayInfo) => Promise<unknown>;
}): Promise<void> {
  if (!options.isCurrent()) return;
  if (options.existing) { await options.open(options.existing); return; }
  const created = await options.create();
  for (let attempt = 0; attempt < 8 && options.isCurrent(); attempt += 1) {
    const displays = await options.refresh();
    if (!options.isCurrent()) return;
    const display = displays.find(entry => entry.sourceId === created.sourceId);
    if (display) { await options.open(display); return; }
    if (attempt < 7) await new Promise(resolve => setTimeout(resolve, 500));
  }
  if (options.isCurrent()) throw new LocalizedError("errExtNotListed");
}
