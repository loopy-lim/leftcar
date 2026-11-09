// ── unikit generated ────────────────────────────────────────
// File:   react-native.config.js
// Source: schema.json (single source of truth for this file)
// Regen:  unikit codegen --config unikit.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

module.exports = {
  dependency: {
    platforms: {
      ios: { podspecPath: './UnikitBridge.podspec' },
      android: {
        sourceDir: './android',
        packageImportPath: 'import dev.unikit.bridge.UnikitBridgePackage;',
        packageInstance: 'new UnikitBridgePackage()',
      },
    },
  },
};
