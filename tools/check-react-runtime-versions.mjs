import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const apps = [
  ["Leftcar Host", "../apps/host-desktop/package.json"],
  ["Leftcar Viewer (Expo)", "../apps/viewer-expo/package.json"],
];

const errors = [];

for (const [name, relativeManifestPath] of apps) {
  const manifestUrl = new URL(relativeManifestPath, import.meta.url);
  const manifest = JSON.parse(readFileSync(manifestUrl, "utf8"));
  const requireFromApp = createRequire(manifestUrl);
  const reactVersion = requireFromApp("react/package.json").version;
  const reactDomVersion = requireFromApp("react-dom/package.json").version;
  const declaredReact = manifest.dependencies?.react;
  const declaredReactDom = manifest.dependencies?.["react-dom"];

  if (declaredReact !== reactVersion || declaredReactDom !== reactDomVersion) {
    errors.push(
      `${name}: React dependencies must be exact installed versions ` +
        `(declared react=${declaredReact}, react-dom=${declaredReactDom}; ` +
        `installed react=${reactVersion}, react-dom=${reactDomVersion})`,
    );
  }

  if (reactVersion !== reactDomVersion) {
    errors.push(
      `${name}: incompatible React renderer versions ` +
        `(react=${reactVersion}, react-dom=${reactDomVersion})`,
    );
  }
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}

console.log("React runtime versions are exact and compatible (19.2.3).");
