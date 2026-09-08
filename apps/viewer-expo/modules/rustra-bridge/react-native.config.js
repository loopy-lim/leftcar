// ── rustra generated ────────────────────────────────────────
// File:   react-native.config.js
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

module.exports = {
  dependency: {
    platforms: {
      ios: { podspecPath: './RustraBridge.podspec' },
      android: {
        sourceDir: './android',
        packageImportPath: 'import dev.rustra.bridge.RustraBridgePackage;',
        packageInstance: 'new RustraBridgePackage()',
      },
    },
  },
};
