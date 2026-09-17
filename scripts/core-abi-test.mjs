#!/usr/bin/env node
/**
 * Headless test of the libretro-in-wasm contract, across every built core.
 *
 * Runs each core with its own test ROM in plain Node — no browser, no GPU, no Rust —
 * and asserts what actually came out:
 *
 *   instantiate → shim_install → retro_init → load ROM → retro_run x N
 *   → video_refresh / audio_batch / input_state callbacks
 *
 * This is the fast loop. A browser run takes ~25 s and can fail for a dozen unrelated
 * reasons; this takes about a second and only fails when a core contract is broken.
 *
 * The test ROMs are ours (`scripts/make-test-rom.py`, `scripts/make-gba-rom.sh`,
 * `scripts/make-sms-rom.py`) and are written to be asserted against. Crucially each
 * uses a *different* idle colour — NES green, GBA blue, Master System magenta — so a
 * test can identify which core produced a frame from the pixels alone. That is what
 * makes the hot-swap checks in `smoke-test.mjs` meaningful rather than a matter of
 * trusting bookkeeping.
 *
 * Usage:
 *   node scripts/core-abi-test.mjs            # every core with a built module
 *   node scripts/core-abi-test.mjs fceumm     # just one
 */

import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { LibretroRuntime, PIXEL_FORMAT } from '../web/src/engine/core-runtime.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const RETRO_DEVICE_JOYPAD = 1;
const BUTTON = { B: 0, Y: 1, SELECT: 2, START: 3, UP: 4, DOWN: 5, LEFT: 6, RIGHT: 7, A: 8 };

/**
 * What each core is expected to do with its ROM.
 *
 * `idle`/`pressed` name the dominant colour channel, which is how a test states
 * "the emulated picture is what the ROM draws" without hard-coding exact palettes that
 * differ slightly between cores' colour conversion.
 */
const CORES = [
  {
    id: 'fceumm',
    label: 'NES',
    module: 'web/cores/fceumm.wasm',
    rom: 'web/roms/nes-testcart.nes',
    extension: 'nes',
    geometry: { width: 256, height: 240 },
    fps: [59, 61],
    idle: 'green',
    pressed: 'red',
    // The tile pattern's border rows are invariant under horizontal scroll, so the
    // movement check must sample a fill row.
    motionScanline: 124,
    motionButton: BUTTON.RIGHT,
    motionFrames: 4,
    buildHint: 'scripts/build-core.sh fceumm',
    romHint: 'python3 scripts/make-test-rom.py',
  },
  {
    id: 'mgba',
    label: 'GBA',
    module: 'web/cores/mgba.wasm',
    rom: 'web/roms/gba-testcart.gba',
    extension: 'gba',
    geometry: { width: 240, height: 160 },
    fps: [59, 61],
    idle: 'blue',
    pressed: 'yellow',
    // The marker block sits at y=72..88 and slides horizontally.
    motionScanline: 80,
    motionButton: BUTTON.RIGHT,
    motionFrames: 30,
    buildHint: 'scripts/build-core.sh mgba',
    romHint: 'scripts/make-gba-rom.sh',
  },
  {
    id: 'genesis_plus_gx',
    label: 'Master System',
    module: 'web/cores/genesis_plus_gx.wasm',
    rom: 'web/roms/sms-testcart.sms',
    extension: 'sms',
    // Genesis Plus GX picks its system from the content extension, so this entry is
    // also the regression test for `full_path` being populated in retro_game_info_ext:
    // a 256x192 frame means Master System, a 320x224 one means it fell back to Mega
    // Drive and is running Z80 code on the 68000.
    geometry: { width: 256, height: 192 },
    fps: [59, 61],
    idle: 'magenta',
    pressed: 'cyan',
    // Tile rows 0 and 7 are the dark border and are invariant under a horizontal
    // scroll; 100 lands on row 4, which has vertical edges to shift.
    motionScanline: 100,
    motionButton: BUTTON.RIGHT,
    motionFrames: 4,
    buildHint: 'scripts/build-core.sh genesis_plus_gx',
    romHint: 'python3 scripts/make-sms-rom.py',
  },
];

let failures = 0;
let checks = 0;

function check(name, ok, detail = '') {
  checks++;
  if (!ok) failures++;
  console.log(`  ${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`);
}

// --------------------------------------------------------------- pixel helpers

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

/** Mean RGB over a grid of samples, ignoring the edges. */
function averageColour(shot) {
  let r = 0;
  let g = 0;
  let b = 0;
  let n = 0;
  const inset = Math.min(16, Math.floor(shot.height / 8));
  for (let y = inset; y < shot.height - inset; y += 3) {
    for (let x = inset; x < shot.width - inset; x += 3) {
      const [pr, pg, pb] = pixelAt(shot, x, y);
      r += pr;
      g += pg;
      b += pb;
      n++;
    }
  }
  return [Math.round(r / n), Math.round(g / n), Math.round(b / n)];
}

/**
 * Classifies an average colour by which channels dominate. Tolerant on purpose: cores
 * differ in how they convert their native colour space, so asserting exact values
 * would make this test about palettes rather than about emulation.
 */
function describeColour([r, g, b]) {
  const margin = 20;
  if (r > g + margin && r > b + margin) return 'red';
  if (g > r + margin && g > b + margin) return 'green';
  if (b > r + margin && b > g + margin) return 'blue';
  if (r > b + margin && g > b + margin) return 'yellow';
  if (r > g + margin && b > g + margin) return 'magenta';
  if (g > r + margin && b > r + margin) return 'cyan';
  return `mixed(${r},${g},${b})`;
}

// -------------------------------------------------------------------- one core

async function testCore(core) {
  console.log(`\n── ${core.label} (${core.id})`);

  const modulePath = path.join(ROOT, core.module);
  const romPath = path.join(ROOT, core.rom);
  if (!existsSync(modulePath)) {
    check(`${core.id} module present`, false, `missing ${core.module}; build with: ${core.buildHint}`);
    return;
  }
  if (!existsSync(romPath)) {
    check(`${core.id} test ROM present`, false, `missing ${core.rom}; generate with: ${core.romHint}`);
    return;
  }

  let frame = null;
  let frameCount = 0;
  let audioFrames = 0;
  let audioPeak = 0;
  const held = new Set();

  const runtime = await LibretroRuntime.instantiate(readFileSync(modulePath), {
    video: ({ data, width, height, pitch, format }) => {
      // Copied out of core memory so it stays valid after `run()` returns.
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
    log: (message) => process.env.CORE_VERBOSE && console.log('     ' + message),
  });

  const runFrames = (count) => {
    for (let i = 0; i < count; i++) runtime.run();
  };

  const info = runtime.systemInfo;
  check('core reports system info', Boolean(info.name), `${info.name} ${info.version}`);
  check(
    `accepts .${core.extension} content from memory (content-info override honoured)`,
    runtime.needsFullpath(core.extension) === false,
    `extensions: ${info.validExtensions.join(', ')}; ${runtime.contentOverrides.length} override group(s)`,
  );

  const rom = readFileSync(romPath);
  const av = runtime.loadGame(rom, core.extension, path.basename(romPath, `.${core.extension}`));

  check(
    'core loads the ROM and reports geometry',
    av.geometry.baseWidth === core.geometry.width && av.geometry.baseHeight === core.geometry.height,
    `${av.geometry.baseWidth}x${av.geometry.baseHeight}, max ${av.geometry.maxWidth}x${av.geometry.maxHeight}, ` +
      `aspect ${av.geometry.aspectRatio.toFixed(3)}`,
  );
  check(
    'core reports plausible timing',
    av.timing.fps > core.fps[0] && av.timing.fps < core.fps[1] && av.timing.sampleRate > 8000,
    `${av.timing.fps.toFixed(4)} fps, ${av.timing.sampleRate} Hz`,
  );
  check(
    'core negotiated a supported pixel format',
    runtime.pixelFormat === PIXEL_FORMAT.RGB565 || runtime.pixelFormat === PIXEL_FORMAT.XRGB8888,
    runtime.pixelFormat === PIXEL_FORMAT.RGB565 ? 'RGB565' : 'XRGB8888',
  );

  // Enough frames to get past any start-up transient.
  runFrames(40);

  check('core produced frames', frameCount >= 30, `${frameCount} video_refresh calls`);
  check(
    'frame geometry matches the reported geometry',
    frame && frame.width === core.geometry.width && frame.height === core.geometry.height,
    frame ? `${frame.width}x${frame.height}, pitch ${frame.pitch}` : 'no frame',
  );

  const expectedAudioPerFrame = av.timing.sampleRate / av.timing.fps;
  check(
    'core produced audible audio at the expected rate',
    audioPeak > 0 && Math.abs(audioFrames / frameCount - expectedAudioPerFrame) < 80,
    `${audioFrames} frames over ${frameCount} video frames ` +
      `(${(audioFrames / frameCount).toFixed(0)}/frame, expected ~${expectedAudioPerFrame.toFixed(0)}), ` +
      `peak amplitude ${audioPeak}`,
  );

  const idle = frame;
  const idleColour = averageColour(idle);
  check(
    `ROM renders its ${core.idle} pattern (CPU + video emulation working)`,
    describeColour(idleColour) === core.idle,
    `average rgb ${idleColour.join(',')} → ${describeColour(idleColour)}`,
  );

  // A bordered tile pattern must produce both dark and bright pixels; a uniform frame
  // would mean the pattern was never drawn.
  let dark = 0;
  let bright = 0;
  for (let y = 8; y < idle.height - 8; y++) {
    for (let x = 8; x < idle.width - 8; x++) {
      const [r, g, b] = pixelAt(idle, x, y);
      if (r + g + b < 90) dark++;
      else bright++;
    }
  }
  check(
    'tile pattern is rendered, not a flat fill',
    dark > 500 && bright > 500,
    `${dark} dark / ${bright} bright pixels`,
  );

  held.add(BUTTON.A);
  runFrames(6);
  const pressedColour = averageColour(frame);
  check(
    `input reaches the core (A held → ${core.pressed})`,
    describeColour(pressedColour) === core.pressed,
    `average rgb ${pressedColour.join(',')} → ${describeColour(pressedColour)}`,
  );

  held.delete(BUTTON.A);
  runFrames(6);
  check(
    'releasing the button restores the previous state',
    describeColour(averageColour(frame)) === core.idle,
    `average rgb ${averageColour(frame).join(',')}`,
  );

  // Per-frame register/VRAM writes: the picture must move while a direction is held.
  const before = frame;
  held.add(core.motionButton);
  runFrames(core.motionFrames);
  held.delete(core.motionButton);
  const after = frame;
  let changed = 0;
  for (let x = 8; x < before.width - 8; x++) {
    const [r1, g1, b1] = pixelAt(before, x, core.motionScanline);
    const [r2, g2, b2] = pixelAt(after, x, core.motionScanline);
    if (Math.abs(r1 - r2) + Math.abs(g1 - g2) + Math.abs(b1 - b2) > 60) changed++;
  }
  check(
    'held direction moves the picture (per-frame writes land)',
    changed > 10,
    `${changed} pixels changed along scanline ${core.motionScanline}`,
  );

  const stateSize = runtime.serializeSize();
  check('core supports save states', stateSize > 0, `${stateSize} bytes`);

  if (stateSize > 0) {
    const saved = runtime.serialize();
    held.add(BUTTON.A);
    runFrames(10);
    held.delete(BUTTON.A);
    const diverged = describeColour(averageColour(frame));

    runtime.unserialize(saved);
    runFrames(3);
    const restored = describeColour(averageColour(frame));
    check(
      'save state round-trips through the core',
      diverged === core.pressed && restored === core.idle,
      `diverged ${diverged} → restored ${restored}`,
    );
  }

  runtime.destroy();
  check('core tears down cleanly', true);

  return { frameCount, audioFrames };
}

// ------------------------------------------------------------------------ main

const requested = process.argv[2];
const selected = requested ? CORES.filter((c) => c.id === requested) : CORES;
if (selected.length === 0) {
  console.error(`error: unknown core '${requested}'. Known: ${CORES.map((c) => c.id).join(', ')}`);
  process.exit(1);
}

for (const core of selected) {
  await testCore(core);
}

console.log(
  `\n── ${checks - failures}/${checks} checks passed across ${selected.length} core(s)` +
    (failures ? `, ${failures} FAILED` : '') +
    '\n',
);
process.exit(failures === 0 ? 0 : 1);
