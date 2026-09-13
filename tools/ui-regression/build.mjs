import { mkdir, writeFile } from "node:fs/promises";
const outdir = process.env.UI_TEST_DIR ?? "/tmp/leftcar-task4-ui";
await mkdir(outdir, { recursive: true });
const result = await Bun.build({
  entrypoints: [`${import.meta.dir}/entry.jsx`],
  outdir,
  target: "browser",
});
if (!result.success) throw new Error(result.logs.join("\n"));
await writeFile(
  `${outdir}/index.html`,
  '<!doctype html><html><head><meta charset="utf-8"><title>Isolated UI regression</title></head><body><div id="root"></div><script src="entry.js"></script></body></html>',
);

const viewer = await Bun.build({
  entrypoints: [`${import.meta.dir}/catalog.jsx`],
  outdir,
  target: "browser",
  format: "iife",
  plugins: [
    {
      name: "controlled-device-io",
      setup(build) {
        build.onResolve(
          {
            filter:
              /^(react-native|expo-secure-store|expo-router|expo-clipboard|expo-constants|expo-crypto)$/,
          },
          () => ({ path: `${import.meta.dir}/viewer-io.js` }),
        );
        build.onResolve({ filter: /^react-native-tcp-socket$/ }, () => ({
          path: `${import.meta.dir}/tcp-io.js`,
        }));
      },
    },
  ],
});
if (!viewer.success) throw new Error(viewer.logs.join("\n"));
await writeFile(
  `${outdir}/catalog.html`,
  '<!doctype html><html><head><meta charset="utf-8"><title>Isolated catalog lifecycle</title></head><body><div id="root"></div><script src="catalog.js"></script></body></html>',
);

const grants = await Bun.build({ entrypoints: [`${import.meta.dir}/source-grants.jsx`], outdir, target: "browser" });
if (!grants.success) throw new Error(grants.logs.join("\n"));
await writeFile(`${outdir}/source-grants.html`, '<!doctype html><html><head><meta charset="utf-8"><title>Isolated Host source approval</title></head><body><div id="root"></div><script src="source-grants.js"></script></body></html>');

const pairingGrants = await Bun.build({entrypoints:[`${import.meta.dir}/pairing-grants.jsx`],outdir,target:"browser"});
if(!pairingGrants.success)throw new Error(pairingGrants.logs.join("\n"));
await writeFile(`${outdir}/pairing-grants.html`,'<!doctype html><html><head><meta charset="utf-8"><title>Isolated actual pairing parent</title></head><body><div id="root"></div><script src="pairing-grants.js"></script></body></html>');
const camera=await Bun.build({entrypoints:[`${import.meta.dir}/camera.jsx`],outdir,target:'browser',format:'iife',plugins:[{name:'camera-os-only',setup(build){
 build.onResolve({filter:/^(react-native|expo-camera|expo-linking|react-native-safe-area-context|expo-router|@expo\/vector-icons)$/},()=>({path:`${import.meta.dir}/camera-io.jsx`}));
 build.onResolve({filter:/^(expo-secure-store|expo-clipboard|expo-constants|expo-crypto)$/},()=>({path:`${import.meta.dir}/viewer-io.js`}));
 build.onResolve({filter:/^react-native-tcp-socket$/},()=>({path:`${import.meta.dir}/tcp-io.js`}));
}}]});if(!camera.success)throw new Error(camera.logs.join('\n'));
await writeFile(`${outdir}/camera.html`,'<!doctype html><meta charset="utf-8"><div id="root"></div><script src="camera.js"></script>');

const host = await Bun.build({ entrypoints: [`${import.meta.dir}/host.jsx`], outdir, target: 'browser', format: 'iife', plugins: [{ name: 'host-os-and-transport-only', setup(build) {
  build.onResolve({ filter: /^(react-native|react-native-safe-area-context|expo-router|@expo\/vector-icons)$/ }, () => ({path: `${import.meta.dir}/host-io.jsx`}));
  build.onResolve({ filter: /^(expo-clipboard|expo-constants|expo-crypto)$/ }, () => ({path: `${import.meta.dir}/viewer-io.js`}));
  build.onResolve({ filter: /^expo-secure-store$/ }, () => ({path: `${import.meta.dir}/host-storage-io.js`}));
  build.onResolve({ filter: /^\.\/control$/ }, args => args.importer.endsWith('/src/session.ts') ? ({path: `${import.meta.dir}/host-control-io.js`}) : undefined);
  build.onResolve({ filter: /^react-native-tcp-socket$/ }, () => ({path: `${import.meta.dir}/tcp-io.js`}));
} }] });
if (!host.success) throw new Error(host.logs.join('\n'));
await writeFile(`${outdir}/host.html`, '<!doctype html><meta charset="utf-8"><div id="root"></div><script src="host.js"></script>');

const hubConnect = await Bun.build({ entrypoints: [`${import.meta.dir}/hub-connect.jsx`], outdir, target: 'browser', format: 'iife', plugins: [{ name: 'hub-os-and-transport-only', setup(build) {
  build.onResolve({ filter: /^(react-native|react-native-safe-area-context|@expo\/vector-icons)$/ }, () => ({path: `${import.meta.dir}/host-io.jsx`}));
  build.onResolve({ filter: /^expo-router$/ }, () => ({path: `${import.meta.dir}/hub-connect-io.jsx`}));
  build.onResolve({ filter: /^(expo-clipboard|expo-constants|expo-crypto)$/ }, () => ({path: `${import.meta.dir}/viewer-io.js`}));
  build.onResolve({ filter: /^expo-secure-store$/ }, () => ({path: `${import.meta.dir}/host-storage-io.js`}));
  build.onResolve({ filter: /^\.\/control$/ }, args => args.importer.endsWith('/src/session.ts') ? ({path: `${import.meta.dir}/host-control-io.js`}) : undefined);
  build.onResolve({ filter: /^react-native-tcp-socket$/ }, () => ({path: `${import.meta.dir}/tcp-io.js`}));
} }] });
if (!hubConnect.success) throw new Error(hubConnect.logs.join('\n'));
await writeFile(`${outdir}/hub-connect.html`, '<!doctype html><meta charset="utf-8"><div id="root"></div><script src="hub-connect.js"></script>');
