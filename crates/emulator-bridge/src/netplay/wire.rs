//! The netplay wire format: what one peer says to the other, as bytes.
//!
//! Pure data, no I/O. The Swift side moves these bytes over a TCP `NWConnection` and never looks
//! inside them, so Android can carry the same bytes over its own sockets and talk to an iPhone.
//!
//! ## Framing
//!
//! TCP is a byte stream, so every message is length-prefixed:
//!
//! ```text
//!   [u32 little-endian: length of everything after this field] [u8 tag] [payload]
//! ```
//!
//! Integers are little-endian. A string is a `u16` byte length then UTF-8. A byte blob is a
//! `u32` length then the bytes. The decoder is incremental: feed it whatever arrived, take whole
//! messages out, and a message split across two reads simply waits for the rest.
//!
//! ## Inputs are quantised on BOTH sides
//!
//! Lockstep only works if both cores see bit-identical input. The local pad is a set of floats
//! (stick positions, a touch point), so the local peer does not feed its own floats to its core:
//! it encodes them to [`WireInput`] and decodes them back, exactly as the remote peer will. Both
//! cores then read the same quantised values.

use crate::input::{PortState, AXIS_COUNT};

/// Bumped whenever a message changes shape. A peer with a different number is refused at the
/// handshake with a readable reason instead of misparsing frames later.
pub const PROTOCOL_VERSION: u16 = 1;

/// Four bytes at the start of `Hello`, so a stray connection from something that is not
/// Continuum is refused immediately.
pub const MAGIC: [u8; 4] = *b"CNTM";

/// Ceiling on one message. A save-state chunk is the largest legitimate message, and chunks are
/// [`STATE_CHUNK_BYTES`], so anything far beyond that is a corrupt or hostile stream.
pub const MAX_MESSAGE_BYTES: usize = STATE_CHUNK_BYTES + 1024;

/// Size of one save-state chunk. Small enough that a chunk never holds the socket for long,
/// large enough that a 4 MB PlayStation state is about a hundred messages.
pub const STATE_CHUNK_BYTES: usize = 48 * 1024;

/// One player's controller for one frame, in a form that is identical on both peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WireInput {
    /// Bitfield indexed by [`crate::input::Button`].
    pub buttons: u16,
    /// Analog axes scaled to `-32767..=32767`.
    pub axes: [i16; AXIS_COUNT],
    /// Pointer as a fraction of the framebuffer scaled to `0..=65535`.
    pub pointer: [u16; 2],
    pub pointer_pressed: bool,
}

impl WireInput {
    /// Quantises a live port. The ONLY way local input enters a netplay frame.
    pub fn from_port(port: &PortState) -> Self {
        let mut axes = [0i16; AXIS_COUNT];
        for (dst, src) in axes.iter_mut().zip(port.axes.iter()) {
            let v = if src.is_finite() { *src } else { 0.0 };
            *dst = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        }
        let quantise = |f: f32| -> u16 {
            let f = if f.is_finite() { f } else { 0.0 };
            (f.clamp(0.0, 1.0) * 65535.0).round() as u16
        };
        Self {
            buttons: (port.buttons & 0xFFFF) as u16,
            axes,
            pointer: [quantise(port.pointer[0]), quantise(port.pointer[1])],
            pointer_pressed: port.pointer_pressed,
        }
    }

    /// Expands back into the port state the core reads. Deterministic: the same `WireInput`
    /// always produces bit-identical floats.
    pub fn to_port(self) -> PortState {
        let mut axes = [0f32; AXIS_COUNT];
        for (dst, src) in axes.iter_mut().zip(self.axes.iter()) {
            *dst = f32::from(*src) / 32767.0;
        }
        PortState {
            buttons: u32::from(self.buttons),
            axes,
            pointer: [
                f32::from(self.pointer[0]) / 65535.0,
                f32::from(self.pointer[1]) / 65535.0,
            ],
            pointer_pressed: self.pointer_pressed,
        }
    }

    const ENCODED_LEN: usize = 2 + 2 * AXIS_COUNT + 4 + 1;

    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.buttons.to_le_bytes());
        for axis in self.axes {
            out.extend_from_slice(&axis.to_le_bytes());
        }
        out.extend_from_slice(&self.pointer[0].to_le_bytes());
        out.extend_from_slice(&self.pointer[1].to_le_bytes());
        out.push(u8::from(self.pointer_pressed));
    }

    fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let buttons = r.u16()?;
        let mut axes = [0i16; AXIS_COUNT];
        for axis in &mut axes {
            *axis = r.u16()? as i16;
        }
        let pointer = [r.u16()?, r.u16()?];
        let pointer_pressed = r.u8()? != 0;
        Ok(Self {
            buttons,
            axes,
            pointer,
            pointer_pressed,
        })
    }
}

/// Everything a guest says about itself when it connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub version: u16,
    pub core_id: String,
    pub core_version: String,
    /// The ROM's filename. Shown in a refusal so the user sees which game the other phone has.
    pub content_name: String,
    /// See [`super::content_fingerprint`].
    pub content_fingerprint: u64,
    pub state_size: u64,
    /// Enabled cheats. Lockstep with different cheats is a guaranteed desync, so any is refused.
    pub active_cheats: u32,
    /// Hash of every core option's current value (see [`super::options_hash`]). Two cores set up
    /// differently (region, renderer, enhancements) split apart within seconds.
    pub options_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Guest to host, first thing after connecting.
    Hello(PeerInfo),
    /// Host to guest: accepted. The state follows in chunks.
    Welcome {
        input_delay: u8,
        checksum_interval: u32,
        state_len: u64,
        state_hash: u64,
    },
    /// Either way: refused, with a reason a person can read.
    Reject {
        reason: String,
    },
    StateChunk {
        offset: u64,
        bytes: Vec<u8>,
    },
    /// Guest to host: the state is loaded and frame 0 can begin.
    Ready,
    /// One player's input for one frame.
    Input {
        frame: u64,
        input: WireInput,
    },
    /// Hash of the whole serialized state after `frame` frames have run.
    Checksum {
        frame: u64,
        hash: u64,
    },
    Ping {
        token: u64,
    },
    Pong {
        token: u64,
    },
    /// Clean goodbye.
    Bye {
        reason: String,
    },
}

mod tag {
    pub const HELLO: u8 = 1;
    pub const WELCOME: u8 = 2;
    pub const REJECT: u8 = 3;
    pub const STATE_CHUNK: u8 = 4;
    pub const READY: u8 = 5;
    pub const INPUT: u8 = 6;
    pub const CHECKSUM: u8 = 7;
    pub const PING: u8 = 8;
    pub const PONG: u8 = 9;
    pub const BYE: u8 = 10;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("message too large ({0} bytes)")]
    TooLarge(usize),
    #[error("message ended early")]
    Truncated,
    #[error("unknown message type {0}")]
    UnknownTag(u8),
    #[error("text in a message was not UTF-8")]
    BadText,
    #[error("not a Continuum connection")]
    BadMagic,
    #[error("{0} trailing bytes after a message")]
    Trailing(usize),
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    // Strings here are ids and reasons, never long; truncation at a char boundary keeps the
    // u16 length honest rather than panicking on a pathological reason string.
    let mut end = s.len().min(u16::MAX as usize);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    out.extend_from_slice(&(end as u16).to_le_bytes());
    out.extend_from_slice(&s.as_bytes()[..end]);
}

impl Message {
    /// Appends this message, framed, to `out`.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0, 0, 0, 0]);
        match self {
            Message::Hello(info) => {
                out.push(tag::HELLO);
                out.extend_from_slice(&MAGIC);
                out.extend_from_slice(&info.version.to_le_bytes());
                put_str(out, &info.core_id);
                put_str(out, &info.core_version);
                put_str(out, &info.content_name);
                out.extend_from_slice(&info.content_fingerprint.to_le_bytes());
                out.extend_from_slice(&info.state_size.to_le_bytes());
                out.extend_from_slice(&info.active_cheats.to_le_bytes());
                out.extend_from_slice(&info.options_hash.to_le_bytes());
            }
            Message::Welcome {
                input_delay,
                checksum_interval,
                state_len,
                state_hash,
            } => {
                out.push(tag::WELCOME);
                out.push(*input_delay);
                out.extend_from_slice(&checksum_interval.to_le_bytes());
                out.extend_from_slice(&state_len.to_le_bytes());
                out.extend_from_slice(&state_hash.to_le_bytes());
            }
            Message::Reject { reason } => {
                out.push(tag::REJECT);
                put_str(out, reason);
            }
            Message::StateChunk { offset, bytes } => {
                out.push(tag::STATE_CHUNK);
                out.extend_from_slice(&offset.to_le_bytes());
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(bytes);
            }
            Message::Ready => out.push(tag::READY),
            Message::Input { frame, input } => {
                out.push(tag::INPUT);
                out.extend_from_slice(&frame.to_le_bytes());
                input.encode(out);
            }
            Message::Checksum { frame, hash } => {
                out.push(tag::CHECKSUM);
                out.extend_from_slice(&frame.to_le_bytes());
                out.extend_from_slice(&hash.to_le_bytes());
            }
            Message::Ping { token } => {
                out.push(tag::PING);
                out.extend_from_slice(&token.to_le_bytes());
            }
            Message::Pong { token } => {
                out.push(tag::PONG);
                out.extend_from_slice(&token.to_le_bytes());
            }
            Message::Bye { reason } => {
                out.push(tag::BYE);
                put_str(out, reason);
            }
        }
        let len = (out.len() - start - 4) as u32;
        out[start..start + 4].copy_from_slice(&len.to_le_bytes());
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    /// Decodes one message body (tag and payload, without the length prefix).
    fn decode_body(body: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader { buf: body, pos: 0 };
        let tag = r.u8()?;
        let message = match tag {
            tag::HELLO => {
                let magic = r.take(4)?;
                if magic != MAGIC {
                    return Err(WireError::BadMagic);
                }
                Message::Hello(PeerInfo {
                    version: r.u16()?,
                    core_id: r.string()?,
                    core_version: r.string()?,
                    content_name: r.string()?,
                    content_fingerprint: r.u64()?,
                    state_size: r.u64()?,
                    active_cheats: r.u32()?,
                    options_hash: r.u64()?,
                })
            }
            tag::WELCOME => Message::Welcome {
                input_delay: r.u8()?,
                checksum_interval: r.u32()?,
                state_len: r.u64()?,
                state_hash: r.u64()?,
            },
            tag::REJECT => Message::Reject {
                reason: r.string()?,
            },
            tag::STATE_CHUNK => {
                let offset = r.u64()?;
                let len = r.u32()? as usize;
                Message::StateChunk {
                    offset,
                    bytes: r.take(len)?.to_vec(),
                }
            }
            tag::READY => Message::Ready,
            tag::INPUT => {
                if r.remaining() < 8 + WireInput::ENCODED_LEN {
                    return Err(WireError::Truncated);
                }
                Message::Input {
                    frame: r.u64()?,
                    input: WireInput::decode(&mut r)?,
                }
            }
            tag::CHECKSUM => Message::Checksum {
                frame: r.u64()?,
                hash: r.u64()?,
            },
            tag::PING => Message::Ping { token: r.u64()? },
            tag::PONG => Message::Pong { token: r.u64()? },
            tag::BYE => Message::Bye {
                reason: r.string()?,
            },
            other => return Err(WireError::UnknownTag(other)),
        };
        if r.remaining() != 0 {
            return Err(WireError::Trailing(r.remaining()));
        }
        Ok(message)
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        if self.remaining() < n {
            return Err(WireError::Truncated);
        }
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, WireError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, WireError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, WireError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
    fn string(&mut self) -> Result<String, WireError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| WireError::BadText)
    }
}

/// Incremental decoder over a TCP byte stream.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Bytes received and not yet consumed as a whole message.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// The next complete message, `Ok(None)` if more bytes are needed.
    pub fn next_message(&mut self) -> Result<Option<Message>, WireError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > MAX_MESSAGE_BYTES {
            return Err(WireError::TooLarge(len));
        }
        if len == 0 {
            return Err(WireError::Truncated);
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let message = Message::decode_body(&self.buf[4..4 + len]);
        self.buf.drain(..4 + len);
        message.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_messages() -> Vec<Message> {
        vec![
            Message::Hello(PeerInfo {
                version: PROTOCOL_VERSION,
                core_id: "snes9x".into(),
                core_version: "1.62".into(),
                content_name: "Game (USA).sfc".into(),
                content_fingerprint: 0xDEAD_BEEF_0123_4567,
                state_size: 400_000,
                active_cheats: 0,
                options_hash: 7,
            }),
            Message::Welcome {
                input_delay: 2,
                checksum_interval: 60,
                state_len: 123,
                state_hash: 99,
            },
            Message::Reject {
                reason: "different game".into(),
            },
            Message::StateChunk {
                offset: 48,
                bytes: vec![1, 2, 3, 4, 5],
            },
            Message::Ready,
            Message::Input {
                frame: 7,
                input: WireInput {
                    buttons: 0x0123,
                    axes: [-32767, 0, 5, 32767],
                    pointer: [0, 65535],
                    pointer_pressed: true,
                },
            },
            Message::Checksum {
                frame: 60,
                hash: 42,
            },
            Message::Ping { token: 1 },
            Message::Pong { token: 1 },
            Message::Bye {
                reason: "left".into(),
            },
        ]
    }

    #[test]
    fn every_message_round_trips() {
        for message in all_messages() {
            let bytes = message.encode();
            let mut decoder = Decoder::new();
            decoder.push(&bytes);
            assert_eq!(decoder.next_message().unwrap(), Some(message));
            assert_eq!(decoder.pending(), 0);
        }
    }

    #[test]
    fn a_stream_split_one_byte_at_a_time_still_decodes() {
        let mut stream = Vec::new();
        for message in all_messages() {
            message.encode_into(&mut stream);
        }
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        for byte in stream {
            decoder.push(&[byte]);
            while let Some(m) = decoder.next_message().unwrap() {
                out.push(m);
            }
        }
        assert_eq!(out, all_messages());
    }

    #[test]
    fn an_oversized_length_is_refused_before_buffering_it() {
        let mut decoder = Decoder::new();
        decoder.push(&(u32::MAX).to_le_bytes());
        assert!(matches!(
            decoder.next_message(),
            Err(WireError::TooLarge(_))
        ));
    }

    #[test]
    fn a_hello_without_the_magic_is_not_continuum() {
        let mut bytes = all_messages()[0].encode();
        bytes[5] = b'X';
        let mut decoder = Decoder::new();
        decoder.push(&bytes);
        assert_eq!(decoder.next_message(), Err(WireError::BadMagic));
    }

    #[test]
    fn quantised_input_is_stable_through_a_second_round_trip() {
        let port = PortState {
            buttons: 0b1010_0000_0001,
            axes: [0.333, -0.9999, 1.5, f32::NAN],
            pointer: [0.25, 0.75],
            pointer_pressed: true,
        };
        let once = WireInput::from_port(&port);
        let twice = WireInput::from_port(&once.to_port());
        // The local peer feeds `once.to_port()` to its core and the remote peer feeds the
        // decoded copy of `once`; both must be the same bits.
        assert_eq!(once, twice);
        assert_eq!(once.axes[2], 32767, "clamped");
        assert_eq!(once.axes[3], 0, "NaN becomes centred");
    }
}
