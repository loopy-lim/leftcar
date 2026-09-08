import type { AddNumbersInput, AddNumbersOutput } from './types.js';
import { createGeneratedFields2, invokeGenerated } from '@rustra/types';
import type { InvokeOptions } from '@rustra/types';

export const addNumbers = createGeneratedFields2<AddNumbersInput, AddNumbersOutput>(1, 'addNumbers', "a", "b", 'addNumbers');

