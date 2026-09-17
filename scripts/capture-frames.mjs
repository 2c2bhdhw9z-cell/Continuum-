#!/usr/bin/env node
/**
 * Writes PNGs of what each core actually renders, straight from its framebuffer.
 *
 * Screenshotting the app in CI would be misleading: headless Chromium on SwiftShader
 * cannot composite a WebGPU canvas, so the canvas reads back empty and the result looks
 * like a broken application rather than a limitation of the environment. These images
 * come from the core's own output instead — the same bytes the renderer uploads — so they
 * are honest documentation and they regenerate on demand.
 *
 * Usage: node scripts/capture-frames.mjs [outputDir]
 */

import { readFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { LibretroRuntime, PIXEL_FORMAT } from '../web/src/engine/core-runtime.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT_DIR = process.argv[2] ?? path.join(ROOT, 'docs');

const RETRO_DEVICE_JOYPAD = 1;
const BUTTON_A = 8;

const TARGETS = [
  {
    name: 'frame-nes',
    label: 'NES / fceumm',
    module: 'web/cores/fceumm.wasm',
    rom: 'web/roms/nes-testcart.nes',
    extension: 'nes',
    /** Scale up so 256x240 is legible in a README. Integer, to keep pixels crisp. */
    scale: 2,
  },
  {
    name: 'frame-gba',
    label: 'GBA / mGBA',
    module: 'web/cores/mgba.wasm',
    rom: 'web/roms/gba-testcart.gba',
    extension: 'gba',
    scale: 2,
  },
  {
    name: 'frame-sms',
    label: 'Master System / Genesis Plus GX',
    module: 'web/cores/genesis_plus_gx.wasm',
    rom: 'web/roms/sms-testcart.sms',
    // The extension is load-bearing for this core: it is how Genesis Plus GX decides
    // to be a Master System rather than a Mega Drive.
    extension: 'sms',
    scale: 2,
  },
  {
    name: 'frame-snes',
    label: 'SNES / Snes9x',
    module: 'web/cores/snes9x.wasm',
    rom: 'web/roms/snes-testcart.sfc',
    extension: 'sfc',
    scale: 2,
  },
];

// ------------------------------------------------------------------ PNG writing

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

/** Minimal RGB PNG encoder: filter type 0 per scanline, one IDAT. */
function encodePng(rgb, width, height) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 2; // colour type: truecolour
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    const rowStart = y * (width * 3 + 1);
    raw[rowStart] = 0; // no filter
    rgb.copy(raw, rowStart + 1, y * width * 3, (y + 1) * width * 3);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

// ------------------------------------------------------------- frame conversion

/** Core framebuffer → tightly packed RGB, nearest-neighbour scaled. */
function toRgb(frame, scale) {
  const { data, width, height, pitch, format } = frame;
  const out = Buffer.alloc(width * scale * height * scale * 3);
  for (let y = 0; y < height * scale; y++) {
    const sourceY = Math.floor(y / scale);
    for (let x = 0; x < width * scale; x++) {
      const sourceX = Math.floor(x / scale);
      let r;
      let g;
      let b;
      if (format === PIXEL_FORMAT.RGB565) {
        const offset = sourceY * pitch + sourceX * 2;
        const value = data[offset] | (data[offset + 1] << 8);
        const r5 = (value >> 11) & 0x1f;
        const g6 = (value >> 5) & 0x3f;
        const b5 = value & 0x1f;
        r = (r5 << 3) | (r5 >> 2);
        g = (g6 << 2) | (g6 >> 4);
        b = (b5 << 3) | (b5 >> 2);
      } else {
        const offset = sourceY * pitch + sourceX * 4; // XRGB8888: B G R X
        r = data[offset + 2];
        g = data[offset + 1];
        b = data[offset];
      }
      const target = (y * width * scale + x) * 3;
      out[target] = r;
      out[target + 1] = g;
      out[target + 2] = b;
    }
  }
  return out;
}

// ------------------------------------------------------------------------ main

mkdirSync(OUT_DIR, { recursive: true });
let written = 0;

for (const target of TARGETS) {
  const modulePath = path.join(ROOT, target.module);
  const romPath = path.join(ROOT, target.rom);
  if (!existsSync(modulePath) || !existsSync(romPath)) {
    console.log(`skip ${target.name}: build ${target.module} and ${target.rom} first`);
    continue;
  }

  let frame = null;
  const held = new Set();
  const runtime = await LibretroRuntime.instantiate(readFileSync(modulePath), {
    video: ({ data, width, height, pitch, format }) => {
      frame = { data: Uint8Array.from(data), width, height, pitch, format };
    },
    audio: () => {},
    inputState: (port, device, _index, id) =>
      port === 0 && device === RETRO_DEVICE_JOYPAD && held.has(id) ? 1 : 0,
  });

  runtime.loadGame(readFileSync(romPath), target.extension, target.name);
  // Long enough for the ROM to finish drawing and settle.
  for (let i = 0; i < 40; i++) runtime.run();

  const idle = Buffer.from(toRgb(frame, target.scale));
  writeFileSync(
    path.join(OUT_DIR, `${target.name}.png`),
    encodePng(idle, frame.width * target.scale, frame.height * target.scale),
  );
  console.log(
    `wrote ${target.name}.png — ${target.label}, ${frame.width}x${frame.height} ` +
      `(x${target.scale})`,
  );

  // A second image with A held, which is what the input assertions key on.
  held.add(BUTTON_A);
  for (let i = 0; i < 6; i++) runtime.run();
  const pressed = Buffer.from(toRgb(frame, target.scale));
  writeFileSync(
    path.join(OUT_DIR, `${target.name}-a.png`),
    encodePng(pressed, frame.width * target.scale, frame.height * target.scale),
  );
  console.log(`wrote ${target.name}-a.png — same frame with A held`);

  runtime.destroy();
  written += 2;
}

console.log(`\n${written} image(s) written to ${path.relative(ROOT, OUT_DIR)}/`);
