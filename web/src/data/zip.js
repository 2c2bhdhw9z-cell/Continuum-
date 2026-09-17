/**
 * A ZIP reader, in about two hundred lines and with no dependency.
 *
 * ## Why not fflate
 *
 * fflate is a fine library, and importing it would mean vendoring ~30 KB of someone
 * else's DEFLATE implementation to do something the browser already does. `Compression­
 * Stream`/`DecompressionStream` have shipped everywhere this app can run — they are how
 * `png.js` already produces its thumbnails — and `'deflate-raw'` is exactly the variant a
 * ZIP entry stores: raw DEFLATE with no zlib wrapper.
 *
 * So what is actually missing is not decompression, it is the *container*: a few
 * fixed-layout records and a directory walk. That is what this file is.
 *
 * ## Reading a ZIP backwards
 *
 * ZIP is designed to be read from the end, because it was designed for floppies spanning
 * several disks. The authoritative index is the central directory, whose location is
 * given by the End Of Central Directory record at the very end of the file:
 *
 * ```text
 *   [local header][data] [local header][data] ... [central directory] [EOCD]
 *                                                        ▲                │
 *                                                        └────────────────┘
 * ```
 *
 * Reading the central directory rather than scanning local headers matters for
 * correctness, not just speed: a local header is allowed to carry zeroes for the sizes
 * and CRC when the archive was written as a stream, with the real values following the
 * data in a descriptor. The central directory always has them.
 *
 * ## What is deliberately not supported
 *
 * - **Encrypted entries.** Reported as such rather than producing garbage bytes.
 * - **Compression methods other than store (0) and deflate (8).** Every ROM archive in
 *   the wild uses one of those two; bzip2/LZMA/zstd entries are named in the error.
 * - **ZIP64.** Guarded explicitly: an archive with the ZIP64 sentinel values is refused
 *   with a clear message instead of reading a truncated offset. A single ROM never needs
 *   it — the format's limits are 4 GB per entry and 65,535 entries.
 */

import { crc32 } from './crc32.js';

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;

/** Fixed sizes of the records this reader walks. */
const EOCD_MIN_SIZE = 22;
const CENTRAL_HEADER_SIZE = 46;
const LOCAL_HEADER_SIZE = 30;

/** The comment field is 16-bit, so the EOCD starts at most this far from the end. */
const MAX_COMMENT = 0xffff;

const METHOD_STORE = 0;
const METHOD_DEFLATE = 8;

/** Method numbers worth naming when refusing, so the message is actionable. */
const METHOD_NAMES = {
  1: 'shrink',
  6: 'implode',
  9: 'deflate64',
  12: 'bzip2',
  14: 'LZMA',
  93: 'zstd',
  95: 'XZ',
  98: 'PPMd',
};

/** ZIP64 sentinel: a field of all ones means "the real value is in an extra field". */
const ZIP64_SENTINEL = 0xffffffff;

export function looksLikeZip(bytes) {
  // "PK\3\4" — the first local header. Checked rather than trusting the extension,
  // because that is the house style for content detection here.
  return (
    bytes.length > 4 && bytes[0] === 0x50 && bytes[1] === 0x4b && bytes[2] === 0x03 && bytes[3] === 0x04
  );
}

/**
 * @typedef {{
 *   name: string, compressedSize: number, size: number, method: number,
 *   crc: number, offset: number, encrypted: boolean, directory: boolean,
 * }} ZipEntry
 */

/**
 * Reads the central directory.
 *
 * @param {Uint8Array} bytes the whole archive
 * @returns {ZipEntry[]}
 */
export function listEntries(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

  // Scan backwards for the EOCD signature. It is not at a fixed offset because the
  // archive may carry a trailing comment.
  const limit = Math.max(0, bytes.length - EOCD_MIN_SIZE - MAX_COMMENT);
  let eocd = -1;
  for (let i = bytes.length - EOCD_MIN_SIZE; i >= limit; i--) {
    if (view.getUint32(i, true) === EOCD_SIGNATURE) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) {
    throw new Error('not a valid zip archive (no end-of-central-directory record)');
  }

  const count = view.getUint16(eocd + 10, true);
  const directorySize = view.getUint32(eocd + 12, true);
  const directoryOffset = view.getUint32(eocd + 16, true);

  if (directoryOffset === ZIP64_SENTINEL || directorySize === ZIP64_SENTINEL || count === 0xffff) {
    throw new Error('zip64 archives are not supported');
  }
  if (directoryOffset + directorySize > bytes.length) {
    throw new Error('zip central directory is outside the file (truncated download?)');
  }

  const entries = [];
  let p = directoryOffset;
  for (let i = 0; i < count; i++) {
    if (p + CENTRAL_HEADER_SIZE > bytes.length) break;
    if (view.getUint32(p, true) !== CENTRAL_SIGNATURE) {
      throw new Error(`zip central directory entry ${i} has a bad signature`);
    }

    const flags = view.getUint16(p + 8, true);
    const method = view.getUint16(p + 10, true);
    const crc = view.getUint32(p + 16, true);
    const compressedSize = view.getUint32(p + 20, true);
    const size = view.getUint32(p + 24, true);
    const nameLength = view.getUint16(p + 28, true);
    const extraLength = view.getUint16(p + 30, true);
    const commentLength = view.getUint16(p + 32, true);
    const offset = view.getUint32(p + 42, true);

    const nameBytes = bytes.subarray(p + CENTRAL_HEADER_SIZE, p + CENTRAL_HEADER_SIZE + nameLength);
    // Bit 11 promises UTF-8. Without it the spec says CP437, but every modern writer
    // uses UTF-8 anyway and ROM filenames are overwhelmingly ASCII, so decoding as
    // UTF-8 either way is the pragmatic choice — and `fatal: false` means a genuinely
    // CP437 name with accents degrades to replacement characters instead of throwing.
    const name = new TextDecoder('utf-8', { fatal: false }).decode(nameBytes);

    entries.push({
      name,
      compressedSize,
      size,
      method,
      crc,
      offset,
      // Bit 0 is the traditional PKWARE encryption flag; bit 6 is strong encryption.
      encrypted: (flags & 0x1) !== 0 || (flags & 0x40) !== 0,
      directory: name.endsWith('/') || name.endsWith('\\'),
    });

    p += CENTRAL_HEADER_SIZE + nameLength + extraLength + commentLength;
  }

  return entries;
}

/**
 * Extracts one entry.
 *
 * @param {Uint8Array} bytes the whole archive
 * @param {ZipEntry} entry
 * @param {{verify?: boolean}} [options] `verify` checks the CRC; on by default
 * @returns {Promise<Uint8Array>}
 */
export async function extract(bytes, entry, { verify = true } = {}) {
  if (entry.encrypted) {
    throw new Error(`"${entry.name}" is encrypted; extract it yourself and import the ROM`);
  }
  if (entry.directory) throw new Error(`"${entry.name}" is a directory`);

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (entry.offset + LOCAL_HEADER_SIZE > bytes.length) {
    throw new Error(`"${entry.name}" points past the end of the archive`);
  }
  if (view.getUint32(entry.offset, true) !== LOCAL_SIGNATURE) {
    throw new Error(`"${entry.name}" has no local header where the directory says it is`);
  }

  // The local header's own name/extra lengths are what locate the data. They are
  // allowed to differ from the central directory's, so they must be read here rather
  // than reused.
  const nameLength = view.getUint16(entry.offset + 26, true);
  const extraLength = view.getUint16(entry.offset + 28, true);
  const start = entry.offset + LOCAL_HEADER_SIZE + nameLength + extraLength;
  const end = start + entry.compressedSize;
  if (end > bytes.length) {
    throw new Error(`"${entry.name}" is truncated (${entry.compressedSize} bytes claimed)`);
  }

  const compressed = bytes.subarray(start, end);

  let out;
  if (entry.method === METHOD_STORE) {
    // `slice`, not `subarray`: the caller gets bytes it owns, and the archive — which
    // may be hundreds of megabytes — becomes collectable immediately.
    out = compressed.slice();
  } else if (entry.method === METHOD_DEFLATE) {
    out = await inflateRaw(compressed, entry.size);
  } else {
    const label = METHOD_NAMES[entry.method] ?? `method ${entry.method}`;
    throw new Error(`"${entry.name}" uses ${label} compression, which this build cannot read`);
  }

  if (entry.size && out.length !== entry.size) {
    throw new Error(
      `"${entry.name}" inflated to ${out.length} bytes, but the directory says ${entry.size}`,
    );
  }
  if (verify) {
    const actual = crc32(out);
    if (actual !== entry.crc) {
      // Worth failing on rather than warning about: a ROM with one flipped byte boots
      // and then misbehaves somewhere unrelated, which is a miserable thing to debug.
      throw new Error(
        `"${entry.name}" failed its checksum (expected ${entry.crc.toString(16)}, ` +
          `got ${actual.toString(16)}) — the archive is corrupt`,
      );
    }
  }
  return out;
}

/**
 * Raw DEFLATE through the platform.
 *
 * `'deflate-raw'` rather than `'deflate'`: a ZIP entry holds bare DEFLATE with no zlib
 * header or Adler-32 trailer, and feeding it to the wrapped variant fails on the first
 * two bytes.
 */
async function inflateRaw(compressed, expectedSize) {
  if (typeof DecompressionStream !== 'function') {
    throw new Error('this browser cannot inflate zip entries (DecompressionStream missing)');
  }
  const stream = new DecompressionStream('deflate-raw');
  const writer = stream.writable.getWriter();
  // Not awaited before close: the reader below is what drains the pipe, and awaiting
  // the write first deadlocks on any entry larger than the internal queue.
  void writer.write(compressed);
  void writer.close();

  const buffer = await new Response(stream.readable).arrayBuffer();
  const out = new Uint8Array(buffer);
  if (expectedSize && out.length !== expectedSize) return out; // reported by the caller
  return out;
}

/**
 * Picks the one entry from an archive that is actually a ROM.
 *
 * Archives from the wild are messier than "one file, one ROM": they carry readmes,
 * `__MACOSX/` resource forks, nested folders, and occasionally several regional dumps
 * of the same game. So the rule is explicit rather than "take the first file":
 *
 *   1. Ignore directories, macOS metadata and dotfiles.
 *   2. Prefer an entry whose extension the ROM detector recognises.
 *   3. Among those, prefer the largest — a `.nes` beside a 2 KB `.txt` is unambiguous,
 *      and for two real ROMs the larger is the better guess than the alphabetically
 *      first.
 *   4. With no recognised extension at all, fall back to the largest file, because
 *      header sniffing downstream may still identify it.
 *
 * @param {ZipEntry[]} entries
 * @param {string[]} knownExtensions e.g. `['.nes', '.sfc']`
 * @returns {{entry: ZipEntry|null, candidates: ZipEntry[], ignored: number}}
 */
export function pickRomEntry(entries, knownExtensions) {
  const known = new Set(knownExtensions.map((e) => e.replace(/^\./, '').toLowerCase()));

  const usable = [];
  let ignored = 0;
  for (const entry of entries) {
    const base = entry.name.split('/').pop() ?? entry.name;
    if (
      entry.directory ||
      entry.size === 0 ||
      base.startsWith('.') ||
      entry.name.startsWith('__MACOSX/')
    ) {
      ignored++;
      continue;
    }
    usable.push(entry);
  }

  const byExtension = usable.filter((entry) => {
    const ext = entry.name.split('.').pop()?.toLowerCase() ?? '';
    return known.has(ext);
  });

  if (byExtension.length) {
    const entry = byExtension.slice().sort((a, b) => b.size - a.size)[0];
    return { entry, candidates: byExtension, ignored };
  }

  // No recognised extension. The fallback is still worth trying, because header
  // sniffing downstream identifies plenty of files with odd or missing extensions —
  // but not when the only thing in the archive is obviously documentation. Extracting
  // a readme and then reporting "could not identify this ROM" describes the wrong
  // problem to someone whose archive simply has no ROM in it.
  const documentation = usable.filter((entry) => {
    const ext = entry.name.split('.').pop()?.toLowerCase() ?? '';
    return NON_ROM_EXTENSIONS.has(ext);
  });
  const plausible = usable.filter((entry) => !documentation.includes(entry));

  const entry = plausible.slice().sort((a, b) => b.size - a.size)[0] ?? null;
  return { entry, candidates: [], ignored: ignored + documentation.length };
}

/** Extensions that are never a ROM, so an archive of only these has none. */
const NON_ROM_EXTENSIONS = new Set([
  'txt', 'nfo', 'md', 'diz', 'doc', 'docx', 'pdf', 'rtf', 'html', 'htm', 'url',
  'png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp',
  'sav', 'srm', 'state', 'cht', 'ips', 'bps', 'ups', 'xdelta',
  'zip', 'rar', '7z', 'tar', 'gz',
  'mp3', 'ogg', 'wav', 'flac', 'mp4', 'avi', 'mkv',
  'exe', 'dll', 'bat', 'sh',
]);
