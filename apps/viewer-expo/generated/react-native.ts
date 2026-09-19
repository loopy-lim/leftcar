// ── rustra generated ────────────────────────────────────────
// File:   react-native.ts
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → host entry
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

import { createRustraBootstrap } from '@rustra/react-native';
import { installRustraJSI, getRustraNative } from "@rustra/generated-react-native";
import { GENERATED_CONTRACT_HASH, SCHEMA_VERSION } from './contract.js';
import { frameRegistry } from './frame-registry.js';

export * from './commands.js';
export { subscribeEvent } from '@rustra/react-native';

export const rustra = createRustraBootstrap({
  install: installRustraJSI,
  getNative: getRustraNative,
  frameCodecs: frameRegistry,
  contractHash: GENERATED_CONTRACT_HASH,
  contractVerification: 'strict',
  schemaVersion: SCHEMA_VERSION,
});
