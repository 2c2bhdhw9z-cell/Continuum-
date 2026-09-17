/**
 * Identifies which system a ROM belongs to.
 *
 * Extension alone is unreliable — `.bin` covers Genesis, PS1 and half a dozen
 * others, and a mislabelled `.smc` is common — so headers are checked first and the
 * extension is only a fallback. Getting this wrong means loading a ROM into the
 * wrong core, which fails in confusing ways deep inside emulation.
 *
 * Every check is bounded to the first few kilobytes; nothing here scans a whole file.
 */

/** Fallback map, used when no header matches. */
const EXTENSION_MAP = {
  nes: 'nes',
  fds: 'nes',
  unf: 'nes',
  unif: 'nes',
  sfc: 'snes',
  smc: 'snes',
  fig: 'snes',
  swc: 'snes',
  gb: 'gb',
  gbc: 'gbc',
  gba: 'gba',
  sms: 'sms',
  gg: 'sms',
  md: 'genesis',
  gen: 'genesis',
  smd: 'genesis',
  n64: 'n64',
  z64: 'n64',
  v64: 'n64',
  cue: 'ps1',
  chd: 'ps1',
  pbp: 'ps1',
  iso: 'ps1',
};

function ascii(bytes, offset, length) {
  let out = '';
  for (let i = 0; i < length; i++) {
    const byte = bytes[offset + i];
    if (byte === undefined) return out;
    out += String.fromCharCode(byte);
  }
  return out;
}

/** The 8-byte prefix of the Nintendo logo present in every GB and GBA header. */
function hasNintendoLogo(bytes, offset, expected) {
  for (let i = 0; i < expected.length; i++) {
    if (bytes[offset + i] !== expected[i]) return false;
  }
  return true;
}

const GB_LOGO_PREFIX = [0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b];
const GBA_LOGO_PREFIX = [0x24, 0xff, 0xae, 0x51, 0x69, 0x9a, 0xa2, 0x21];

/**
 * @param {Uint8Array} bytes
 * @param {string} filename
 * @returns {{systemId: string|null, extension: string, confidence: 'header'|'extension'|'none', detail: string}}
 */
export function detectSystem(bytes, filename = '') {
  const extension = (filename.split('.').pop() ?? '').toLowerCase();

  // --- iNES / NES 2.0: "NES\x1a" ---
  if (ascii(bytes, 0, 4) === 'NES\x1a') {
    const prgBanks = bytes[4];
    const mapper = ((bytes[6] >> 4) & 0x0f) | (bytes[7] & 0xf0);
    return {
      systemId: 'nes',
      extension: extension || 'nes',
      confidence: 'header',
      detail: `iNES header, mapper ${mapper}, ${prgBanks * 16} KB PRG`,
    };
  }

  // --- Game Boy / Color: logo at 0x104, colour flag at 0x143 ---
  if (hasNintendoLogo(bytes, 0x104, GB_LOGO_PREFIX)) {
    const cgbFlag = bytes[0x143];
    const isColor = cgbFlag === 0x80 || cgbFlag === 0xc0;
    const title = ascii(bytes, 0x134, 11).replace(/\0.*$/, '').trim();
    return {
      systemId: isColor ? 'gbc' : 'gb',
      extension: extension || (isColor ? 'gbc' : 'gb'),
      confidence: 'header',
      detail: `Game Boy header${title ? ` "${title}"` : ''}${isColor ? ', CGB' : ''}`,
    };
  }

  // --- Game Boy Advance: logo at 0x04 ---
  if (hasNintendoLogo(bytes, 0x04, GBA_LOGO_PREFIX)) {
    const title = ascii(bytes, 0xa0, 12).replace(/\0.*$/, '').trim();
    return {
      systemId: 'gba',
      extension: extension || 'gba',
      confidence: 'header',
      detail: `GBA header${title ? ` "${title}"` : ''}`,
    };
  }

  // --- Mega Drive / Genesis: "SEGA" near 0x100 ---
  const segaTag = ascii(bytes, 0x100, 16);
  if (segaTag.startsWith('SEGA') || segaTag.includes('SEGA')) {
    return {
      systemId: 'genesis',
      extension: extension || 'md',
      confidence: 'header',
      detail: `Mega Drive header (${segaTag.trim().slice(0, 16)})`,
    };
  }

  // --- Master System / Game Gear: "TMR SEGA" at one of three offsets ---
  for (const offset of [0x1ff0, 0x3ff0, 0x7ff0]) {
    if (ascii(bytes, offset, 8) === 'TMR SEGA') {
      return {
        systemId: 'sms',
        extension: extension || 'sms',
        confidence: 'header',
        detail: `Sega 8-bit header at 0x${offset.toString(16)}`,
      };
    }
  }

  // --- Nintendo 64: byte order tells us which dump variant this is ---
  const magic =
    (bytes[0] << 24) | (bytes[1] << 16) | (bytes[2] << 8) | bytes[3];
  if (magic === 0x80371240 || magic === 0x37804012 || magic === 0x40123780) {
    return {
      systemId: 'n64',
      extension: extension || 'z64',
      confidence: 'header',
      detail: magic === 0x80371240 ? 'N64 big-endian (z64)' : 'N64 byte-swapped dump',
    };
  }

  // --- SNES: no magic, so use the header checksum pair at either mapping ---
  for (const [offset, layout] of [
    [0x7fdc, 'LoROM'],
    [0xffdc, 'HiROM'],
  ]) {
    const checksum = bytes[offset + 2] | (bytes[offset + 3] << 8);
    const complement = bytes[offset] | (bytes[offset + 1] << 8);
    // A valid SNES header stores a value and its ones-complement.
    if (checksum !== 0 && (checksum ^ complement) === 0xffff) {
      return {
        systemId: 'snes',
        extension: extension || 'sfc',
        confidence: 'header',
        detail: `SNES ${layout} header`,
      };
    }
  }

  const fallback = EXTENSION_MAP[extension];
  if (fallback) {
    return {
      systemId: fallback,
      extension,
      confidence: 'extension',
      detail: `matched by .${extension} extension; no recognised header`,
    };
  }

  return {
    systemId: null,
    extension,
    confidence: 'none',
    detail: extension
      ? `unrecognised .${extension} file`
      : 'no extension and no recognised header',
  };
}

/** Extensions worth offering in a file picker. */
export function acceptedExtensions() {
  return Object.keys(EXTENSION_MAP).map((e) => `.${e}`);
}
