// ── unikit generated ────────────────────────────────────────
// File:   types.ts
// Source: schema.json (single source of truth for this file)
// Regen:  unikit codegen --config unikit.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

export type { EngineClient, UnikitError } from '@unikit-bridge/types';
export { UnikitCommandError } from '@unikit-bridge/types';

/**
 * The canonical H02 proof command: invoked through the real Rustra package invocation path, 20 + 22 must equal 42 (docs/08 H02 수용 기준).
 */
export type AddNumbersInput = {
  a: number | bigint;
  b: number | bigint;
};

export type AddNumbersOutput = {
  value: number | bigint;
};
