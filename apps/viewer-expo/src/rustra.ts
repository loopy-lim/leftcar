/**
 * Rustra 0.8 generated entry point.
 *
 * Importing this module installs the lazy zero-config bootstrap. The native
 * JSI module is still installed only when a generated command is invoked, so
 * app startup does not pay the bridge setup cost or require Expo Go support.
 */
import { rustra } from "../generated/react-native";
import { addNumbers } from "../generated/commands";
export type { AddNumbersInput, AddNumbersOutput } from "../generated/types";

const generatedRustra = Object.freeze({
  bootstrap: rustra,
  commands: { addNumbers },
});

/** Keep the generated Rustra entry reachable from the real app root. */
export function initializeRustra() {
  return generatedRustra;
}
