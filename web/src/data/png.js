/**
 * A PNG encoder, in about a hundred lines and with no canvas.
 *
 * ## Why not just use a canvas
 *
 * The obvious way to turn RGBA bytes into a PNG is `putImageData` on a 2D context
 * followed by `toBlob`. This project's second architectural rule is that rendering
 * goes through WebGPU and a 2D canvas context is never created — the browser suite
 * asserts it by hooking `getContext`. Reaching for a 2D context to make a thumbnail
 * would technically slip past that hook (it only watches `HTMLCanvasElement`, and an
 * `OffscreenCanvas` would not trip it), which is exactly the reason not to do it: an
 * invariant that holds only where it happens to be measured is not an invariant.
 *
 * Encoding by hand is not the sacrifice it sounds like. PNG's structure is four
 * chunks, its only mandatory compression is DEFLATE, and the platform already ships
 * DEFLATE in `CompressionStream`. The result is a real `image/png` blob that goes
 * straight into IndexedDB and displays through `URL.createObjectURL`.
 *
 * ## Structure
 *
 * ```text
 *   signature  89 50 4E 47 0D 0A 1A 0A
 *   IHDR       width, height, bit depth 8, colour type 6 (RGBA), no interlace
 *   IDAT       zlib stream of filter-0 scanlines
 *   IEND
 * ```
 *
 * Filter 0 (None) for every scanline. The adaptive filters exist to help compression
 * on photographic data; emulator output is flat-shaded sprite work with large runs of
 * identical pixels, which DEFLATE already handles well, and choosing filters per line
 * would mean five trial encodes per row for a thumbnail nobody measures the size of.
 */

/** Standard CRC-32 (IEEE 802.3), built once. */
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
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) crc = CRC_TABLE[(crc ^ bytes[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

/** Adler-32, needed for the zlib trailer in the fallback path. */
function adler32(bytes) {
  let a = 1;
  let b = 0;
  for (let i = 0; i < bytes.length; i++) {
    a = (a + bytes[i]) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

/**
 * DEFLATE with a zlib wrapper.
 *
 * `CompressionStream('deflate')` produces exactly the zlib-wrapped stream (RFC 1950)
 * that a PNG `IDAT` requires — `'deflate-raw'` would be the wrong one. Where it is
 * unavailable the fallback emits *stored* blocks: a valid, legal, entirely
 * uncompressed DEFLATE stream. A thumbnail comes out roughly four times larger, which
 * is a far better outcome than no thumbnail.
 */
async function zlibDeflate(bytes) {
  if (typeof CompressionStream === 'function') {
    try {
      const stream = new CompressionStream('deflate');
      const writer = stream.writable.getWriter();
      void writer.write(bytes);
      void writer.close();
      return new Uint8Array(await new Response(stream.readable).arrayBuffer());
    } catch (err) {
      console.warn('[png] CompressionStream failed, storing uncompressed', err);
    }
  }
  return storedDeflate(bytes);
}

/** Uncompressed DEFLATE blocks with a zlib header and Adler-32 trailer. */
function storedDeflate(bytes) {
  const MAX = 65535;
  const blocks = Math.max(1, Math.ceil(bytes.length / MAX));
  const out = new Uint8Array(2 + blocks * 5 + bytes.length + 4);
  let p = 0;

  // 0x78 0x01: deflate, 32 KB window, and (0x78<<8 | 0x01) % 31 === 0 as required.
  out[p++] = 0x78;
  out[p++] = 0x01;

  for (let offset = 0; offset < bytes.length || blocks === 1; offset += MAX) {
    const len = Math.min(MAX, bytes.length - offset);
    const last = offset + len >= bytes.length;
    // BTYPE 00 with the remaining bits padded, so the block is byte-aligned.
    out[p++] = last ? 1 : 0;
    out[p++] = len & 0xff;
    out[p++] = (len >>> 8) & 0xff;
    out[p++] = ~len & 0xff;
    out[p++] = (~len >>> 8) & 0xff;
    out.set(bytes.subarray(offset, offset + len), p);
    p += len;
    if (last) break;
  }

  const sum = adler32(bytes);
  out[p++] = (sum >>> 24) & 0xff;
  out[p++] = (sum >>> 16) & 0xff;
  out[p++] = (sum >>> 8) & 0xff;
  out[p++] = sum & 0xff;

  return out.subarray(0, p);
}

/** Builds one chunk: length, type, data, CRC over type+data. */
function chunk(type, data) {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

const SIGNATURE = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

/**
 * Encodes tightly packed RGBA8 as a PNG.
 *
 * @param {Uint8Array} rgba `width * height * 4` bytes, top row first.
 * @param {number} width
 * @param {number} height
 * @returns {Promise<Blob>} an `image/png` blob
 */
export async function encodePng(rgba, width, height) {
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
    throw new Error(`invalid PNG dimensions ${width}x${height}`);
  }
  const expected = width * height * 4;
  if (rgba.length < expected) {
    throw new Error(`expected ${expected} bytes of RGBA for ${width}x${height}, got ${rgba.length}`);
  }

  // Filter byte per scanline, then the row's pixels.
  const stride = width * 4;
  const raw = new Uint8Array((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    const from = y * stride;
    raw[y * (stride + 1)] = 0;
    raw.set(rgba.subarray(from, from + stride), y * (stride + 1) + 1);
  }

  const ihdr = new Uint8Array(13);
  const header = new DataView(ihdr.buffer);
  header.setUint32(0, width);
  header.setUint32(4, height);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: truecolour with alpha
  ihdr[10] = 0; // compression: DEFLATE, the only defined value
  ihdr[11] = 0; // filter method: the only defined value
  ihdr[12] = 0; // interlace: none

  const idat = await zlibDeflate(raw);

  return new Blob(
    [SIGNATURE, chunk('IHDR', ihdr), chunk('IDAT', idat), chunk('IEND', new Uint8Array(0))],
    { type: 'image/png' },
  );
}
