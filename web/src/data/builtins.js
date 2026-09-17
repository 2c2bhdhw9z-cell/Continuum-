/**
 * Content shipped with the app.
 *
 * One cart per real core, because an emulator with an empty library cannot demonstrate
 * that it works and commercial ROMs cannot be bundled. All three are written from
 * scratch in this repository, so they are ours to distribute — and they are built to be
 * verified against, each with a deliberately different idle colour so that a captured
 * frame identifies which core produced it:
 *
 *   NES (fceumm)                  green, red while A is held
 *   GBA (mGBA)                    blue, yellow while A is held
 *   Master System (Genesis Plus)  magenta, cyan while a button is held
 *
 * These are also what the browser smoke test launches, which means CI exercises the
 * real cores rather than a stand-in.
 */

import { addRealEntry } from './catalog.js';

const NES_TEST_CART = {
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

const GBA_TEST_CART = {
  id: 'builtin-gba-testcart',
  title: 'Continuum Test Cart (GBA)',
  systemId: 'gba',
  sizeBytes: 1144,
  filename: 'gba-testcart.gba',
  source: 'builtin',
  url: './roms/gba-testcart.gba',
  blurb:
    'An original GBA ROM written for this project (web/roms/src/gba-testcart.c, built by ' +
    'scripts/make-gba-rom.sh). Runs on the real mGBA core: hold A to repaint the palette ' +
    'yellow, Left/Right to slide the marker, and listen for the 440 Hz tone. Its idle ' +
    'colour is blue where the NES cart is green, so a frame identifies which core drew it.',
};

const SMS_TEST_CART = {
  id: 'builtin-sms-testcart',
  title: 'Continuum Test Cart (Master System)',
  systemId: 'sms',
  sizeBytes: 32768,
  filename: 'sms-testcart.sms',
  source: 'builtin',
  url: './roms/sms-testcart.sms',
  blurb:
    'An original Master System ROM written for this project, hand-assembled by ' +
    'scripts/make-sms-rom.py. Runs on the real Genesis Plus GX core: hold either button ' +
    'to flip the playfield from magenta to cyan, Right to scroll it, and listen for the ' +
    '440 Hz PSG tone. The .sms extension is what tells the core to be a Master System ' +
    'rather than a Mega Drive.',
};

/** Registers built-in content. Idempotent. */
export function registerBuiltins() {
  return [addRealEntry(NES_TEST_CART), addRealEntry(GBA_TEST_CART), addRealEntry(SMS_TEST_CART)];
}

export const TEST_CART_ID = NES_TEST_CART.id;
export const GBA_TEST_CART_ID = GBA_TEST_CART.id;
export const SMS_TEST_CART_ID = SMS_TEST_CART.id;
