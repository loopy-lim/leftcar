import { unikit } from "../generated/react-native";
import { addNumbers } from "../generated/commands";
export type { AddNumbersInput, AddNumbersOutput } from "../generated/types";

const generatedUnikit = Object.freeze({
  bootstrap: unikit,
  commands: { addNumbers },
});

/** Keep the generated Unikit entry reachable from the real app root. */
export function initializeUnikit() {
  return generatedUnikit;
}

export const initializeRustra = initializeUnikit;
