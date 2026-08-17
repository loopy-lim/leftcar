import type { ViewerAddNumbersInput, ViewerAddNumbersOutput } from './types.js';
import { invoke } from '@rustra/types';

export function viewerAddNumbers(input: ViewerAddNumbersInput): Promise<ViewerAddNumbersOutput> {
  return invoke<ViewerAddNumbersOutput>('viewerAddNumbers', input);
}

