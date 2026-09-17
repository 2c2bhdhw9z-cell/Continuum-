#!/usr/bin/env node
/**
 * Headless test of the libretro-in-wasm contract.
 *
 * Runs a real core with a real ROM in plain Node — no browser, no GPU, no Rust — and
 * asserts the pipeline the shim and `LibretroRuntime` define:
 *
 *   instantiate → shim_install → retro_init → load ROM → retro_run x N
 *   → video_refresh / audio_batch / input_state callbacks
 *
 * This is the fast loop. A browser run takes ~20 s and can fail for a dozen unrelated
 * reasons; this takes under a second and only fails when the core contract is broken.
 * It also proves the emulation is genuinely running: the test ROM paints green,
 * repaints red while A is held, and scrolls when Right is held, so the assertions
 * below cannot pass unless the CPU, PPU, controller port and frame path all work.
 *
 * Usage: node scripts/core-abi-test.mjs [core.wasm] [rom.nes]
 */

import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { LibretroRuntime, PIXEL_FORMAT } from '../web/src/engine/core-runtime.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const CORE_PATH = process.argv[2] ?? path.join(ROOT, 'web/cores/fceumm.wasm');
const ROM_PATH = process.argv[3] ?? path.join(ROOT, 'web/roms/nes-testcart.nes');

let failures = 0;
function check(name, ok, detail = '') {
  if (!ok) failures++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`);
}

for (const [label, file] of [['core', CORE_PATH], ['ROM', ROM_PATH]]) {
  if (!existsSync(file)) {
    console.error(`error: ${label} not found at ${file}`);
    console.error(
      label === 'core'
        ? 'Build it with: scripts/build-core.sh fceumm'
        : 'Generate it with: python3 scripts/make-test-rom.py',
    );
    process.exit(1);
  }
}

// ---------------------------------------------------------------- host state

/** Latest frame, copied out of core memory so it stays valid after `run()`. */
let frame = null;
let frameCount = 0;
let audioFrames = 0;
let audioPeak = 0;
/** Buttons the fake controller is holding: `RETRO_DEVICE_ID_JOYPAD_*` → bool. */
const held = new Set();

const RETRO_DEVICE_JOYPAD = 1;
const BUTTON = { B: 0, Y: 1, SELECT: 2, START: 3, UP: 4, DOWN: 5, LEFT: 6, RIGHT: 7, A: 8 };

const runtime = await LibretroRuntime.instantiate(readFileSync(CORE_PATH), {
  video: ({ data, width, height, pitch, format }) => {
    frame = { data: Uint8Array.from(data), width, height, pitch, format };
    frameCount++;
  },
  audio: (samples, frames) => {
    audioFrames += frames;
    for (let i = 0; i < samples.length; i++) {
      const magnitude = Math.abs(samples[i]);
      if (magnitude > audioPeak) audioPeak = magnitude;
    }
  },
  inputState: (port, device, index, id) => {
    if (port !== 0 || device !== RETRO_DEVICE_JOYPAD) return 0;
    return held.has(id) ? 1 : 0;
  },
  log: (message) => process.env.CORE_VERBOSE && console.log('   ' + message),
});

// ------------------------------------------------------------------- metadata

const info = runtime.systemInfo;
check('core reports system info', Boolean(info.name), `${info.name} ${info.version}`);
check(
  'core accepts .nes content from memory (content-info override honoured)',
  runtime.needsFullpath('nes') === false,
  `extensions: ${info.validExtensions.join(', ')}; ` +
    `${runtime.contentOverrides.length} override group(s)`,
);

// ------------------------------------------------------------------ load game

const rom = readFileSync(ROM_PATH);
check(
  'test ROM has an iNES header',
  rom[0] === 0x4e && rom[1] === 0x45 && rom[2] === 0x53 && rom[3] === 0x1a,
  `${rom.length} bytes`,
);

const av = runtime.loadGame(rom, 'nes');
check(
  'core loads the ROM and reports geometry',
  av.geometry.baseWidth === 256 && av.geometry.baseHeight === 240,
  `${av.geometry.baseWidth}x${av.geometry.baseHeight}, max ` +
    `${av.geometry.maxWidth}x${av.geometry.maxHeight}, aspect ${av.geometry.aspectRatio.toFixed(3)}`,
);
check(
  'core reports NES timing',
  av.timing.fps > 59 && av.timing.fps < 61 && av.timing.sampleRate > 8000,
  `${av.timing.fps.toFixed(4)} fps, ${av.timing.sampleRate} Hz`,
);
check(
  'core negotiated a supported pixel format',
  runtime.pixelFormat === PIXEL_FORMAT.RGB565 || runtime.pixelFormat === PIXEL_FORMAT.XRGB8888,
  runtime.pixelFormat === PIXEL_FORMAT.RGB565 ? 'RGB565' : 'XRGB8888',
);

// -------------------------------------------------------------------- helpers

/** Decodes one pixel to 8-bit RGB, whichever format the core negotiated. */
function pixelAt(shot, x, y) {
  const { data, pitch, format } = shot;
  if (format === PIXEL_FORMAT.RGB565) {
    const offset = y * pitch + x * 2;
    const value = data[offset] | (data[offset + 1] << 8);
    const r5 = (value >> 11) & 0x1f;
    const g6 = (value >> 5) & 0x3f;
    const b5 = value & 0x1f;
    return [(r5 << 3) | (r5 >> 2), (g6 << 2) | (g6 >> 4), (b5 << 3) | (b5 >> 2)];
  }
  const offset = y * pitch + x * 4; // XRGB8888, little-endian: B G R X
  return [data[offset + 2], data[offset + 1], data[offset]];
}

/** Mean RGB over a grid of samples, ignoring the overscan edges. */
function averageColour(shot) {
  let r = 0;
  let g = 0;
  let b = 0;
  let n = 0;
  for (let y = 16; y < shot.height - 16; y += 4) {
    for (let x = 16; x < shot.width - 16; x += 4) {
      const [pr, pg, pb] = pixelAt(shot, x, y);
      r += pr;
      g += pg;
      b += pb;
      n++;
    }
  }
  return [Math.round(r / n), Math.round(g / n), Math.round(b / n)];
}

function runFrames(count) {
  for (let i = 0; i < count; i++) runtime.run();
}

// --------------------------------------------------------------- run and check

// The ROM waits two vblanks, uploads the palette and nametable, then loops. A few
// frames is plenty; run 30 so any startup transient is well past.
runFrames(30);

check('core produced frames', frameCount >= 30, `${frameCount} video_refresh calls`);
check(
  'frame geometry matches the reported geometry',
  frame && frame.width === 256 && frame.height === 240,
  frame ? `${frame.width}x${frame.height}, pitch ${frame.pitch}` : 'no frame',
);
// The ROM drives pulse channel 1, so a non-zero peak means APU emulation ran and the
// samples survived the callback. Rate is checked too: 48 kHz at ~60 fps is ~800
// frames per video frame, and a wildly different figure means the batch callback is
// being mis-counted.
const expectedAudioPerFrame = av.timing.sampleRate / av.timing.fps;
check(
  'core produced audible audio at the expected rate',
  audioPeak > 0 && Math.abs(audioFrames / frameCount - expectedAudioPerFrame) < 40,
  `${audioFrames} frames over ${frameCount} video frames ` +
    `(${(audioFrames / frameCount).toFixed(0)}/frame, expected ~${expectedAudioPerFrame.toFixed(0)}), ` +
    `peak amplitude ${audioPeak}`,
);

const idle = frame;
const idleColour = averageColour(idle);
check(
  'ROM renders its green pattern (CPU + PPU + palette all working)',
  idleColour[1] > idleColour[0] + 20 && idleColour[1] > idleColour[2] + 20,
  `average rgb ${idleColour.join(',')}`,
);

// The tile is a bordered square, so a correct PPU produces both dark and bright
// pixels. A uniform frame would mean the pattern tables were ignored.
let dark = 0;
let bright = 0;
for (let y = 16; y < idle.height - 16; y++) {
  for (let x = 16; x < idle.width - 16; x++) {
    const [, g] = pixelAt(idle, x, y);
    if (g < 40) dark++;
    else bright++;
  }
}
check(
  'tile pattern is rendered, not a flat fill',
  dark > 1000 && bright > 1000,
  `${dark} dark / ${bright} bright pixels`,
);

// Input: holding A repaints the palette entry red, which is a whole-screen change.
held.add(BUTTON.A);
runFrames(5);
const pressedColour = averageColour(frame);
check(
  'input reaches the core (A held → palette turns red)',
  pressedColour[0] > pressedColour[1] + 20,
  `average rgb ${pressedColour.join(',')}`,
);

held.delete(BUTTON.A);
runFrames(5);
const releasedColour = averageColour(frame);
check(
  'releasing the button restores the previous state',
  releasedColour[1] > releasedColour[0] + 20,
  `average rgb ${releasedColour.join(',')}`,
);

// Scrolling proves per-frame register writes land: the pattern shifts horizontally.
//
// Sampled at y=124, which is 4 rows into a tile. The tile's top and bottom rows are
// its black border and are identical across every column, so a border row cannot
// show horizontal movement no matter how well scrolling works.
const SCROLL_SCANLINE = 124;
const before = frame;
held.add(BUTTON.RIGHT);
runFrames(4);
held.delete(BUTTON.RIGHT);
const after = frame;
let changed = 0;
for (let x = 16; x < before.width - 16; x++) {
  const [, g1] = pixelAt(before, x, SCROLL_SCANLINE);
  const [, g2] = pixelAt(after, x, SCROLL_SCANLINE);
  if (Math.abs(g1 - g2) > 30) changed++;
}
check(
  'Right held scrolls the background (per-frame register writes)',
  changed > 20,
  `${changed} pixels shifted along scanline ${SCROLL_SCANLINE}`,
);

// ------------------------------------------------------------- save states

const stateSize = runtime.serializeSize();
check('core supports save states', stateSize > 0, `${stateSize} bytes`);

if (stateSize > 0) {
  const saved = runtime.serialize();
  // Advance with A held so the visible state provably differs from the snapshot.
  held.add(BUTTON.A);
  runFrames(10);
  held.delete(BUTTON.A);
  const divergedColour = averageColour(frame);

  runtime.unserialize(saved);
  runFrames(2);
  const restoredColour = averageColour(frame);
  check(
    'save state round-trips through the core',
    divergedColour[0] > divergedColour[1] && restoredColour[1] > restoredColour[0],
    `diverged ${divergedColour.join(',')} → restored ${restoredColour.join(',')}`,
  );
}

// ------------------------------------------------------------------- teardown

runtime.destroy();
check('core tears down cleanly', true);

console.log(
  `\n── ${failures === 0 ? 'all checks passed' : `${failures} FAILED`} ` +
    `(${frameCount} frames, ${audioFrames} audio frames)\n`,
);
process.exit(failures === 0 ? 0 : 1);
