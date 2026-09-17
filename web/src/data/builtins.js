/**
 * Content shipped with the app.
 *
 * Exactly one entry: the NES test cart built by `scripts/make-test-rom.py`. It exists
 * because an emulator with an empty library cannot demonstrate that it works, and
 * commercial ROMs cannot be bundled. This one is written from scratch in this
 * repository, so it is ours to distribute — and it is built to be verified against:
 * a green tile pattern, red while A is held, scrolling on Left/Right, and a 440 Hz
 * tone.
 *
 * It is also what the browser smoke test launches, which means CI exercises the real
 * fceumm core rather than a stand-in.
 */

import { addRealEntry } from './catalog.js';

const TEST_CART = {
  id: 'builtin-nes-testcart',
  title: 'Continuum Test Cart',
  systemId: 'nes',
  sizeBytes: 24592,
  filename: 'nes-testcart.nes',
  source: 'builtin',
  url: './roms/nes-testcart.nes',
  blurb:
    'An original NES ROM written for this project, assembled by scripts/make-test-rom.py. ' +
    'Renders a tile pattern through the real fceumm core: hold A to repaint it red, ' +
    'Left/Right to scroll, and listen for the 440 Hz square wave. If this runs, the ' +
    'whole pipeline runs.',
};

/** Registers built-in content. Idempotent. */
export function registerBuiltins() {
  return [addRealEntry(TEST_CART)];
}

export const TEST_CART_ID = TEST_CART.id;
