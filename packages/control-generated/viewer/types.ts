export type { EngineClient, RustraError } from '@rustra/types';
export { RustraCommandError } from '@rustra/types';

/**
 * H02/H09 proof command on the viewer path too.
 */
export type ViewerAddNumbersInput = {
  a: number | bigint;
  b: number | bigint;
};

export type ViewerAddNumbersOutput = {
  value: number | bigint;
};

