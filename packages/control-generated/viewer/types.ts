export type { EngineClient, UnikitError } from '@unikit-bridge/types';
export { UnikitCommandError } from '@unikit-bridge/types';

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

