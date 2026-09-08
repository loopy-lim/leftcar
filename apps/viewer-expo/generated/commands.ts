// ── rustra generated ────────────────────────────────────────
// File:   commands.ts
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

import type { AddNumbersInput, AddNumbersOutput } from './types.js';
import { createGeneratedFields2, invokeGenerated } from '@rustra/types';
import type { InvokeOptions } from '@rustra/types';

/**
 * The canonical H02 proof command: invoked through the real Rustra package invocation path, 20 + 22 must equal 42 (docs/08 H02 수용 기준).
 */
export const addNumbers = createGeneratedFields2<AddNumbersInput, AddNumbersOutput>(1, 'addNumbers', "a", "b", 'addNumbers');
