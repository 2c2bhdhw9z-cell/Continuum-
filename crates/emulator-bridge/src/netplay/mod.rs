//! Two-player online play: deterministic lockstep with a small input delay.
//!
//! - [`wire`] is the byte format, with an incremental decoder for a TCP stream.
//! - [`session`] is the state machine: handshake, starting-state transfer, the per-frame input
//!   exchange, checksums, keepalive and every status line.
//! - The engine side (running frames only when the session allows, servicing state requests,
//!   hashing state for checksums, refusing rewind and state loads) is in `bridge.rs`'s
//!   `netplay_glue` child module, because it needs the bridge's private fields.
//!
//! The transport is NOT here. Swift opens a TCP `NWConnection` (or an `NWListener` on the host)
//! and moves opaque bytes in and out through the UniFFI facade. That keeps every rule in Rust,
//! testable with two in-process peers, and reusable by Android over a plain socket.
//!
//! ## Desync detection
//!
//! Every [`session::DEFAULT_CHECKSUM_INTERVAL`] frames both peers hash their whole serialized
//! state ([`fnv1a64`] over `retro_serialize` output) and exchange it. That costs one
//! serialization per second, the same work one rewind snapshot costs, and it needs no core
//! memory map, so this module does not touch `retro_get_memory_data` at all.

pub mod session;
pub mod wire;

pub use session::{
    NetplayConfig, NetplaySession, Phase, Request, Role, StatusKind, Step,
    DEFAULT_CHECKSUM_INTERVAL, DEFAULT_INPUT_DELAY, MAX_INPUT_DELAY,
};
pub use wire::{PeerInfo, WireInput, PROTOCOL_VERSION};

/// TCP port a host listens on. Fixed so it can be typed; the host falls back to any free port
/// (and shows it) if this one is taken.
pub const DEFAULT_PORT: u16 = 55435;

/// Bonjour service type advertised by a host and browsed by a guest. Must match
/// `NSBonjourServices` in the iOS Info.plist.
pub const BONJOUR_SERVICE_TYPE: &str = "_continuum._tcp";

/// 64-bit FNV-1a. The same function `ArtworkDisk.key(forPath:)` uses in Swift. Not
/// cryptographic, and does not need to be: it detects accidental divergence and corruption.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Fast hash of a whole serialized state, for desync checksums and the state transfer check.
///
/// Word-at-a-time rather than [`fnv1a64`]'s byte-at-a-time, because it runs on the display link
/// once a second over states that reach several megabytes (N64, PSP), and byte-wise FNV over
/// those costs a visible fraction of a frame. A multiply-rotate mix per 8-byte word is several
/// times faster and still catches any flipped bit, which is all a desync check needs.
pub fn state_hash(bytes: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ (bytes.len() as u64).wrapping_mul(K);
    let mut words = bytes.chunks_exact(8);
    for word in &mut words {
        let mut w = [0u8; 8];
        w.copy_from_slice(word);
        hash = (hash ^ u64::from_le_bytes(w))
            .wrapping_mul(K)
            .rotate_left(29);
    }
    for byte in words.remainder() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(K).rotate_left(29);
    }
    hash ^ (hash >> 32)
}

/// Hash of a core's current option values, order-independent.
pub fn options_hash(options: &[crate::cores::CoreOption]) -> u64 {
    let mut pairs: Vec<(&str, &str)> = options
        .iter()
        .map(|o| (o.key.as_str(), o.value.as_str()))
        .collect();
    pairs.sort_unstable();
    let mut text = String::new();
    for (key, value) in pairs {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    fnv1a64(text.as_bytes())
}

/// How much of each end of a ROM file the fingerprint reads.
const FINGERPRINT_EDGE_BYTES: u64 = 1024 * 1024;

/// Identifies a ROM file well enough to tell two different games apart, cheaply.
///
/// The size plus the first and last mebibyte. A whole-file hash of a multi-gigabyte PSP or 3DS
/// image on every "Host" tap would take seconds; two games that agree on their size and on both
/// ends are, in practice, the same dump. A file that cannot be read fingerprints as its name, so
/// two phones with the same filename still match and a refusal still names the game.
pub fn content_fingerprint(path: &str) -> u64 {
    use std::io::{Read, Seek, SeekFrom};
    let fallback = || {
        let name = path.rsplit('/').next().unwrap_or(path);
        fnv1a64(name.as_bytes())
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return fallback();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return fallback();
    };
    let mut bytes = len.to_le_bytes().to_vec();
    let head = len.min(FINGERPRINT_EDGE_BYTES) as usize;
    let mut buf = vec![0u8; head];
    if file.read_exact(&mut buf).is_err() {
        return fallback();
    }
    bytes.extend_from_slice(&buf);
    if len > FINGERPRINT_EDGE_BYTES {
        let tail = (len - FINGERPRINT_EDGE_BYTES).min(FINGERPRINT_EDGE_BYTES);
        let mut buf = vec![0u8; tail as usize];
        if file.seek(SeekFrom::End(-(tail as i64))).is_err() || file.read_exact(&mut buf).is_err() {
            return fallback();
        }
        bytes.extend_from_slice(&buf);
    }
    fnv1a64(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_the_swift_artwork_key() {
        // Reference values for 64-bit FNV-1a.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn state_hash_sees_every_byte_and_the_length() {
        let base = vec![0u8; 1003];
        let h = state_hash(&base);
        for i in [0, 7, 8, 500, 1000, 1002] {
            let mut flipped = base.clone();
            flipped[i] ^= 1;
            assert_ne!(state_hash(&flipped), h, "byte {i}");
        }
        assert_ne!(state_hash(&base[..1002]), h);
        assert_eq!(state_hash(&base), h);
    }

    #[test]
    fn options_hash_ignores_order_but_not_values() {
        let opt = |k: &str, v: &str| crate::cores::CoreOption {
            key: k.into(),
            label: String::new(),
            value: v.into(),
            values: Vec::new(),
        };
        let a = [opt("region", "auto"), opt("renderer", "soft")];
        let b = [opt("renderer", "soft"), opt("region", "auto")];
        let c = [opt("renderer", "soft"), opt("region", "pal")];
        assert_eq!(options_hash(&a), options_hash(&b));
        assert_ne!(options_hash(&a), options_hash(&c));
    }

    #[test]
    fn fingerprint_tells_files_apart_and_survives_a_missing_file() {
        let dir = std::env::temp_dir().join(format!("continuum-fp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.sfc");
        let b = dir.join("b.sfc");
        std::fs::write(&a, vec![1u8; 3 * 1024 * 1024]).unwrap();
        let mut other = vec![1u8; 3 * 1024 * 1024];
        *other.last_mut().unwrap() = 2;
        std::fs::write(&b, other).unwrap();
        let fa = content_fingerprint(a.to_str().unwrap());
        assert_eq!(fa, content_fingerprint(a.to_str().unwrap()));
        assert_ne!(fa, content_fingerprint(b.to_str().unwrap()));
        let missing = dir.join("missing.sfc");
        assert_eq!(
            content_fingerprint(missing.to_str().unwrap()),
            fnv1a64(b"missing.sfc")
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
