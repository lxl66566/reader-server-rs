// Shared reader gestures: chrome visibility signal + tap/swipe classification.
// Format readers toggle chrome via setChromeVisible; the shell renders the bars
// from the same signal, so FormatReaderProps stays stable for all readers.

import { createSignal } from 'solid-js';

export const TAP_MAX_MS = 250;
export const TAP_MAX_DIST = 10;

/** whether the reader chrome (bars + panels) is visible */
export const [chromeVisible, setChromeVisible] = createSignal(false);

/** classify a finished pointer gesture as tap or drag */
export function classifyGesture(
  duration: number,
  dx: number,
  dy: number,
): 'drag' | 'tap' {
  if (
    duration < TAP_MAX_MS &&
    Math.abs(dx) < TAP_MAX_DIST &&
    Math.abs(dy) < TAP_MAX_DIST
  ) {
    return 'tap';
  }
  return 'drag';
}
