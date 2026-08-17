import type { AddNumbersInput, AddNumbersOutput } from './types.js';
import { invoke } from '@rustra/types';

export function addNumbers(input: AddNumbersInput): Promise<AddNumbersOutput> {
  return invoke<AddNumbersOutput>('addNumbers', input);
}

