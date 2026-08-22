const { getDefaultConfig } = require("expo/metro-config");
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

module.exports = config;
