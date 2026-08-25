export type { EngineClient, RustraError } from '@rustra/types';
export { RustraCommandError } from '@rustra/types';

/**
 * The canonical H02 proof command: invoked through the real Rustra package invocation path, 20 + 22 must equal 42 (docs/08 H02 수용 기준).
 */
export type AddNumbersInput = {
  a: number;
  b: number;
};

export type AddNumbersOutput = {
  value: number;
};
