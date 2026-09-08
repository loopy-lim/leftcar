import type { ViewerAddNumbersInput, ViewerAddNumbersOutput } from './types.js';
import { createGeneratedFields2, invokeGenerated } from '@rustra/types';
import type { InvokeOptions } from '@rustra/types';

export const viewerAddNumbers = createGeneratedFields2<ViewerAddNumbersInput, ViewerAddNumbersOutput>(1, 'viewerAddNumbers', "a", "b", 'viewerAddNumbers');

