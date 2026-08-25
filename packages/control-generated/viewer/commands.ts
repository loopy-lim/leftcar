import type { ViewerAddNumbersInput, ViewerAddNumbersOutput } from './types.js';
import { createGeneratedFields2, invokeGenerated } from '@rustra/types';
import type { InvokeOptions } from '@rustra/types';

/**
 * H02/H09 proof command on the viewer path too.
 */
export const viewerAddNumbers = createGeneratedFields2<ViewerAddNumbersInput, ViewerAddNumbersOutput>(1, 'viewerAddNumbers', "a", "b", 'viewerAddNumbers');

