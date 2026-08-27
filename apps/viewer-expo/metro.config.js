const { getDefaultConfig } = require("expo/metro-config");
const { withUniwindConfig } = require("uniwind/metro");
const fs = require("node:fs");
const path = require("node:path");

// Bun workspace: Metro가 workspace 패키지(@device-hub/protocol)와 hoisted 의존성을
// 찾도록 루트 node_modules를 watchFolders/nodeModulesPaths에 포함한다.
const config = getDefaultConfig(__dirname);

const monorepoRoot = path.resolve(__dirname, "../..");
config.watchFolders = [monorepoRoot];
config.resolver.nodeModulesPaths = [
  path.resolve(__dirname, "node_modules"),
  path.resolve(__dirname, "../../node_modules"),
];
config.resolver.disableHierarchicalLookup = false;

// generated/rustra 모듈은 TS ESM 규칙('./contract.js')으로 import한다. 이 Metro
// 버전은 extensionAlias를 소비하지 않으므로 resolveRequest에서 상대 경로 .js
// 요청을 같은 이름의 .ts/.tsx 소스로 재작성한다.
const baseResolveRequest = config.resolver.resolveRequest;
config.resolver.resolveRequest = (context, moduleName, platform) => {
  const rewriteTsExt = (name) => {
    if (!name.endsWith(".js") || !name.startsWith(".")) return name;
    const withoutExt = name.slice(0, -3);
    for (const ext of [".ts", ".tsx"]) {
      const candidate = path.resolve(path.dirname(context.originModulePath), `${withoutExt}${ext}`);
      if (fs.existsSync(candidate)) return `${withoutExt}${ext}`;
    }
    return name;
  };
  const rewritten = rewriteTsExt(moduleName);
  if (rewritten !== moduleName) {
    return context.resolveRequest(
      { ...context, resolveRequest: undefined, customResolverOptions: context.customResolverOptions },
      rewritten,
      platform,
    );
  }
  if (baseResolveRequest) {
    return baseResolveRequest(context, moduleName, platform);
  }
  return context.resolveRequest(context, moduleName, platform);
};

module.exports = withUniwindConfig(config, {
  cssEntryFile: "./global.css",
});
