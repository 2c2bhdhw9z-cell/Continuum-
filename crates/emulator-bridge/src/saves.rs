//! Save-state slots and the save-state export file.
//!
//! Two pieces of platform-neutral logic that the iOS store and a future Android store both need,
//! kept here so they cannot drift apart:
//!
//! - [`plan_slots`]: fitting the states a device already has into the fixed 50-slot grid without
//!   losing one.
//! - [`pack_state_export`] / [`unpack_state_export`]: one file that carries a state AND the
//!   metadata the compatibility gate needs, so a state that leaves the device through the share
//!   sheet can be checked as strictly when it comes back as one that never left.
//!
//! The compatibility checks themselves (core id, core version, core options, byte length) stay in
//! the host store, where the live engine is: this module only guarantees the facts survive the
//! round trip.

use std::collections::BTreeMap;

/// How many manual slots each game has.
pub const SLOT_COUNT: u32 = 50;

/// Where one existing state goes in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPlacement {
    /// Already in range and unique: stays where it is.
    Keep { slot: u32 },
    /// Out of range, or sharing a number: moves to this free slot.
    Move { from: i64, to: u32 },
    /// The grid is full. Kept as an extra, listed below the grid and still loadable and
    /// deletable, never deleted by the migration. Only reachable with more than 50 states.
    Overflow { slot: i64 },
}

/// Fits existing manual states into slots `1..=50`.
///
/// `existing` is `(slot number, created_at seconds)` for each manual state of ONE game, in any
/// order. Returns one placement per input, in the same order.
///
/// The rules, in order:
///
/// 1. A state already in `1..=50` keeps its number, so a slot the user knows as "slot 3" is still
///    slot 3 afterwards. If two claim the same number, the newer keeps it.
/// 2. Everything else (numbers above 50 from the old accumulating scheme, zero, negatives, the
///    loser of a tie) moves into the lowest free slot, oldest first, so the ordering the user saw
///    is preserved as far as the grid allows.
/// 3. When the grid is full, the rest are `Overflow`. Nothing is ever dropped: losing a save
///    because a list got a new shape would be the worst bug this code could have.
pub fn plan_slots(existing: &[(i64, f64)]) -> Vec<SlotPlacement> {
    let mut placements: Vec<Option<SlotPlacement>> = vec![None; existing.len()];
    let mut taken = [false; SLOT_COUNT as usize + 1];

    // Rule 1, newest first so a tie goes to the newer.
    let mut by_newest: Vec<usize> = (0..existing.len()).collect();
    by_newest.sort_by(|&a, &b| {
        existing[b]
            .1
            .partial_cmp(&existing[a].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    for &i in &by_newest {
        let slot = existing[i].0;
        if (1..=i64::from(SLOT_COUNT)).contains(&slot) && !taken[slot as usize] {
            taken[slot as usize] = true;
            placements[i] = Some(SlotPlacement::Keep { slot: slot as u32 });
        }
    }

    // Rule 2 and 3, oldest first.
    let mut rest: Vec<usize> = (0..existing.len())
        .filter(|&i| placements[i].is_none())
        .collect();
    rest.sort_by(|&a, &b| {
        existing[a]
            .1
            .partial_cmp(&existing[b].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(existing[a].0.cmp(&existing[b].0))
            .then(a.cmp(&b))
    });
    for i in rest {
        let free = (1..=SLOT_COUNT).find(|&s| !taken[s as usize]);
        placements[i] = Some(match free {
            Some(to) => {
                taken[to as usize] = true;
                SlotPlacement::Move {
                    from: existing[i].0,
                    to,
                }
            }
            None => SlotPlacement::Overflow {
                slot: existing[i].0,
            },
        });
    }

    placements
        .into_iter()
        .map(|p| p.expect("every input was placed by one of the two passes"))
        .collect()
}

/// The metadata that travels with an exported state. Every field the gate checks is here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateExportMeta {
    /// The game's filename, which is the id the host keys states on.
    pub game_id: String,
    pub core_id: Option<String>,
    pub core_version: Option<String>,
    /// Payload length as written. Checked against the payload on import.
    pub byte_count: u64,
    pub frame: u64,
    /// Seconds since 1970.
    pub created_at: f64,
    /// The slot it was in on the device that exported it. Informational on import.
    pub slot: i64,
    /// The user's name for it, may be empty.
    pub label: String,
    /// The core option values the state was saved under, key to value. The host's gate refuses a
    /// state saved under different "restart required" settings, because some cores (Azahar's 3DS
    /// model, audio engine and renderer) crash loading one.
    ///
    /// `None` is UNKNOWN, which is not the same as an empty map: a file written before exports
    /// carried options, or a state whose record never stored them, and the gate falls back to
    /// checking it against defaults exactly as it does an index record with no options. A
    /// `BTreeMap` so the written order is sorted by key and the output is deterministic.
    pub core_options: Option<BTreeMap<String, String>>,
}

/// The first bytes of every export. Sixteen bytes, ending in a newline so `head -c` shows it.
pub const EXPORT_MAGIC: &[u8; 16] = b"CONTINUUM-STATE\n";
const EXPORT_VERSION: u32 = 1;
/// A file that also carries the slot's picture after the payload. Only written when there IS a
/// picture, so a state without one is still a version 1 file every older build reads. An older
/// build given a version 2 file says "update the app" rather than misreading the picture as state.
const EXPORT_VERSION_WITH_PICTURE: u32 = 2;
/// A slot picture is a few hundred kilobytes at most. Anything bigger is left out of the file
/// rather than allowed to make it enormous.
pub const MAX_EXPORT_PICTURE: usize = 8 * 1024 * 1024;
/// A header is a few hundred bytes. Anything claiming more is not one of ours.
const MAX_HEADER: usize = 64 * 1024;

/// Values are written one per line, so a newline or carriage return inside one is escaped, and so
/// is the backslash that does the escaping.
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// A core option line is `core_option=KEY=VALUE`, so the key also escapes `=`: the first `=` that
/// is not escaped is then the separator, whatever the key or the value contain. `unescape` already
/// turns `\=` back into `=`, as it does any escaped character it does not otherwise know.
fn escape_option_key(key: &str) -> String {
    escape(key).replace('=', "\\=")
}

/// Splits a still-escaped `KEY=VALUE` at the first unescaped `=`. `None` when there is none.
fn split_option(raw: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (at, c) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '=' => return Some((&raw[..at], &raw[at + 1..])),
            _ => {}
        }
    }
    None
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// One `.continuumstate` file: magic, version, header length, a `key=value` text header, then the
/// payload exactly as the core wrote it.
///
/// A text header rather than a binary struct, so a file someone has on their computer can be read
/// with a text editor to see which game and core it belongs to. Little-endian lengths.
///
/// Core options, when known, follow the fixed lines: `core_option_count=N`, then one
/// `core_option=KEY=VALUE` per option sorted by key (see [`escape_option_key`]). Still format
/// version 1, because every build that reads version 1 ignores keys it does not know, so an older
/// build imports a newer file and only loses the options. When unknown neither line is written.
pub fn pack_state_export(meta: &StateExportMeta, payload: &[u8]) -> Vec<u8> {
    pack_state_export_with_picture(meta, payload, None)
}

/// [`pack_state_export`], plus the slot's picture (a PNG) after the payload, so the slot shows it
/// the moment the file is imported. `picture_bytes=N` in the header says how long it is, and the
/// file becomes format version 2. An empty or oversized picture is left out, and the file is then
/// exactly what [`pack_state_export`] writes.
pub fn pack_state_export_with_picture(
    meta: &StateExportMeta,
    payload: &[u8],
    picture: Option<&[u8]>,
) -> Vec<u8> {
    let picture = picture.filter(|p| !p.is_empty() && p.len() <= MAX_EXPORT_PICTURE);
    let mut header = String::new();
    let mut line = |key: &str, value: &str| {
        header.push_str(key);
        header.push('=');
        header.push_str(&escape(value));
        header.push('\n');
    };
    line("game_id", &meta.game_id);
    if let Some(core) = &meta.core_id {
        line("core_id", core);
    }
    if let Some(version) = &meta.core_version {
        line("core_version", version);
    }
    // The payload's real length, not whatever the caller thought it was, so a pack cannot write a
    // file that its own unpack would refuse.
    line("byte_count", &payload.len().to_string());
    line("frame", &meta.frame.to_string());
    line("created_at", &meta.created_at.to_string());
    line("slot", &meta.slot.to_string());
    line("label", &meta.label);

    if let Some(options) = &meta.core_options {
        let mut section = format!("core_option_count={}\n", options.len());
        for (key, value) in options {
            section.push_str("core_option=");
            section.push_str(&escape_option_key(key));
            section.push('=');
            section.push_str(&escape(value));
            section.push('\n');
        }
        // Same rule as `byte_count`: never write a file this build's own unpack would refuse. A
        // header past the limit would make the whole state unimportable, so the options are left
        // out instead and the state imports with them unknown, the same as a file from an older
        // build. Real option sets are a few kilobytes, so this is a guard, not a path.
        if header.len() + section.len() <= MAX_HEADER {
            header.push_str(&section);
        }
    }

    // Last, after the options, so a header that had to drop its options still says where the
    // picture is. Never too big to fit: one short line.
    if let Some(picture) = picture {
        header.push_str(&format!("picture_bytes={}\n", picture.len()));
    }
    let version = if picture.is_some() {
        EXPORT_VERSION_WITH_PICTURE
    } else {
        EXPORT_VERSION
    };

    let extra = picture.map_or(0, <[u8]>::len);
    let mut out =
        Vec::with_capacity(EXPORT_MAGIC.len() + 8 + header.len() + payload.len() + extra);
    out.extend_from_slice(EXPORT_MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(payload);
    if let Some(picture) = picture {
        out.extend_from_slice(picture);
    }
    out
}

/// One imported file: the facts, the state, and the slot's picture when the file carried one.
#[derive(Debug, Clone, PartialEq)]
pub struct UnpackedExport {
    pub meta: StateExportMeta,
    pub payload: Vec<u8>,
    pub picture: Option<Vec<u8>>,
}

/// Reads a file written by [`pack_state_export`] and drops any picture.
pub fn unpack_state_export(bytes: &[u8]) -> Result<(StateExportMeta, Vec<u8>), String> {
    unpack_state_export_with_picture(bytes).map(|unpacked| (unpacked.meta, unpacked.payload))
}

/// Reads a file written by [`pack_state_export_with_picture`]. Every failure is a sentence for the
/// status line. A picture that is damaged or cut short costs only the picture, never the state.
pub fn unpack_state_export_with_picture(bytes: &[u8]) -> Result<UnpackedExport, String> {
    if bytes.len() < EXPORT_MAGIC.len() + 8 || &bytes[..EXPORT_MAGIC.len()] != EXPORT_MAGIC {
        return Err(
            "that file is not a Continuum save state export (it does not start with the \
             Continuum header)"
                .into(),
        );
    }
    let at = EXPORT_MAGIC.len();
    let version = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    if version != EXPORT_VERSION && version != EXPORT_VERSION_WITH_PICTURE {
        return Err(format!(
            "that export is format version {version}, and this build reads versions \
             {EXPORT_VERSION} and {EXPORT_VERSION_WITH_PICTURE}; update the app"
        ));
    }
    let header_len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
    if header_len > MAX_HEADER {
        return Err(format!("that export claims a {header_len} byte header, which is damaged"));
    }
    let body = at + 8;
    let Some(header) = bytes.get(body..body + header_len) else {
        return Err("that export is cut short inside its header".into());
    };
    let header = std::str::from_utf8(header)
        .map_err(|_| "that export's header is not text, so it is damaged".to_string())?;
    let rest = &bytes[body + header_len..];

    let mut meta = StateExportMeta::default();
    let mut byte_count: Option<u64> = None;
    let mut picture_bytes: Option<u64> = None;
    let mut option_count: Option<usize> = None;
    let mut options = BTreeMap::new();
    let mut option_lines = 0usize;
    for line in header.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "core_option" {
            // Split before unescaping: the separator is the first `=` the escaping left bare.
            option_lines += 1;
            if let Some((name, setting)) = split_option(value) {
                options.insert(unescape(name), unescape(setting));
            }
            continue;
        }
        let value = unescape(value);
        match key {
            "game_id" => meta.game_id = value,
            "core_id" if !value.is_empty() => meta.core_id = Some(value),
            "core_version" if !value.is_empty() => meta.core_version = Some(value),
            "byte_count" => byte_count = value.parse().ok(),
            "frame" => meta.frame = value.parse().unwrap_or(0),
            "created_at" => meta.created_at = value.parse().unwrap_or(0.0),
            "slot" => meta.slot = value.parse().unwrap_or(0),
            "label" => meta.label = value,
            "core_option_count" => option_count = value.parse().ok(),
            "picture_bytes" => picture_bytes = value.parse().ok(),
            // Unknown keys are a later build's additions, and are ignored rather than refused.
            _ => {}
        }
    }
    // Known only when the count line is there and every option line it promises was read whole and
    // distinct. Anything else (no count: an older build's file; a count that does not match: a
    // damaged or hand-edited header) is UNKNOWN rather than a partial map, the same way the
    // host's index decoder turns a malformed options map into "not recorded": a damaged options
    // section costs the settings check its precision, never the state.
    meta.core_options = match option_count {
        Some(count) if count == option_lines && count == options.len() => Some(options),
        _ => None,
    };
    if meta.game_id.is_empty() {
        return Err("that export does not say which game it belongs to".into());
    }
    let Some(expected) = byte_count else {
        return Err("that export does not record its own length, so it cannot be checked".into());
    };
    // Version 2 puts the picture after the payload. Only there, and only when the header says how
    // long it is, is anything after `byte_count` bytes not state. A picture whose length does not
    // match is dropped and the state still imports.
    let (payload, picture) = match picture_bytes {
        Some(length) if version == EXPORT_VERSION_WITH_PICTURE && rest.len() as u64 >= expected => {
            let (payload, tail) = rest.split_at(expected as usize);
            let fits = length > 0 && tail.len() as u64 == length && tail.len() <= MAX_EXPORT_PICTURE;
            (payload, fits.then(|| tail.to_vec()))
        }
        _ => (rest, None),
    };
    if payload.len() as u64 != expected {
        return Err(format!(
            "that export holds {} byte(s) of state and says it should hold {expected}, so it was \
             cut short or altered",
            payload.len()
        ));
    }
    if payload.is_empty() {
        return Err("that export holds an empty state".into());
    }
    meta.byte_count = expected;
    Ok(UnpackedExport {
        meta,
        payload: payload.to_vec(),
        picture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(plan: &[SlotPlacement]) -> Vec<String> {
        plan.iter()
            .map(|p| match p {
                SlotPlacement::Keep { slot } => format!("keep {slot}"),
                SlotPlacement::Move { from, to } => format!("{from}->{to}"),
                SlotPlacement::Overflow { slot } => format!("over {slot}"),
            })
            .collect()
    }

    #[test]
    fn states_in_range_keep_their_numbers() {
        let plan = plan_slots(&[(1, 10.0), (3, 20.0), (50, 30.0)]);
        assert_eq!(slots(&plan), vec!["keep 1", "keep 3", "keep 50"]);
    }

    #[test]
    fn numbers_past_fifty_fill_the_lowest_gaps_oldest_first() {
        let plan = plan_slots(&[(1, 1.0), (52, 30.0), (51, 20.0), (3, 2.0)]);
        // 51 is older than 52, so it takes slot 2 and 52 takes slot 4.
        assert_eq!(slots(&plan), vec!["keep 1", "52->4", "51->2", "keep 3"]);
    }

    #[test]
    fn a_duplicate_number_goes_to_the_newer_and_the_older_moves() {
        let plan = plan_slots(&[(2, 5.0), (2, 9.0)]);
        assert_eq!(slots(&plan), vec!["2->1", "keep 2"]);
    }

    #[test]
    fn zero_and_negative_numbers_are_moved_not_dropped() {
        let plan = plan_slots(&[(0, 1.0), (-7, 2.0)]);
        assert_eq!(slots(&plan), vec!["0->1", "-7->2"]);
    }

    #[test]
    fn more_than_fifty_overflows_and_loses_nothing() {
        let existing: Vec<(i64, f64)> = (1..=55).map(|n| (n, n as f64)).collect();
        let plan = plan_slots(&existing);
        assert_eq!(plan.len(), 55);
        let kept = plan
            .iter()
            .filter(|p| matches!(p, SlotPlacement::Keep { .. }))
            .count();
        let over = plan
            .iter()
            .filter(|p| matches!(p, SlotPlacement::Overflow { .. }))
            .count();
        assert_eq!((kept, over), (50, 5));
        assert_eq!(plan[54], SlotPlacement::Overflow { slot: 55 });
    }

    #[test]
    fn every_target_slot_is_unique() {
        let existing: Vec<(i64, f64)> = vec![
            (7, 1.0),
            (7, 2.0),
            (100, 0.5),
            (0, 3.0),
            (49, 4.0),
            (50, 5.0),
            (51, 6.0),
        ];
        let plan = plan_slots(&existing);
        let mut targets: Vec<u32> = plan
            .iter()
            .filter_map(|p| match p {
                SlotPlacement::Keep { slot } => Some(*slot),
                SlotPlacement::Move { to, .. } => Some(*to),
                SlotPlacement::Overflow { .. } => None,
            })
            .collect();
        let before = targets.len();
        targets.sort_unstable();
        targets.dedup();
        assert_eq!(targets.len(), before);
        assert_eq!(before, existing.len());
        assert!(targets.iter().all(|&s| (1..=SLOT_COUNT).contains(&s)));
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(plan_slots(&[]).is_empty());
    }

    fn sample_meta() -> StateExportMeta {
        StateExportMeta {
            game_id: "Super Mario World (USA).sfc".into(),
            core_id: Some("snes9x".into()),
            core_version: Some("1.62.3 abc123".into()),
            byte_count: 0,
            frame: 12345,
            created_at: 1_700_000_000.5,
            slot: 7,
            label: "before the castle\nsecond line".into(),
            core_options: None,
        }
    }

    fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The text header of a packed export, for tests that look at the lines themselves.
    fn header_of(bytes: &[u8]) -> &str {
        let len = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        std::str::from_utf8(&bytes[24..24 + len]).unwrap()
    }

    /// Exactly what a build from before exports carried options writes, byte for byte.
    fn old_format_export(payload: &[u8]) -> Vec<u8> {
        let header = format!(
            "game_id=Super Mario World (USA).sfc\ncore_id=snes9x\ncore_version=1.62.3 abc123\n\
             byte_count={}\nframe=12345\ncreated_at=1700000000.5\nslot=7\n\
             label=before the castle\\nsecond line\n",
            payload.len()
        );
        let mut out = EXPORT_MAGIC.to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn export_round_trips_core_options_with_awkward_characters() {
        let mut meta = sample_meta();
        meta.core_options = Some(options(&[
            ("citra_is_new_3ds", "New 3DS"),
            ("a=b", "c=d=e"),
            ("==", "="),
            ("line\nbreak", "value\nwith\r\nnewlines"),
            ("back\\slash\\", "\\"),
            ("ends with escape\\=", "\\n is not a newline"),
            ("", "empty key"),
            ("empty value", ""),
            ("ünïcödé 3DS 🎮", "日本語=はい"),
            // A value that tries to forge a header line must stay a value.
            ("forge", "x\ngame_id=Other Game.sfc\nbyte_count=1"),
        ]));
        let bytes = pack_state_export(&meta, &[4u8; 64]);
        let (back, payload) = unpack_state_export(&bytes).unwrap();
        assert_eq!(payload, vec![4u8; 64]);
        assert_eq!(back.core_options, meta.core_options);
        assert_eq!(back.game_id, "Super Mario World (USA).sfc");
        let mut expected = meta.clone();
        expected.byte_count = 64;
        assert_eq!(back, expected);
    }

    #[test]
    fn an_old_format_export_without_options_unpacks_as_unknown() {
        let payload = [7u8; 300];
        let (meta, back) = unpack_state_export(&old_format_export(&payload)).unwrap();
        assert_eq!(back, payload);
        assert_eq!(meta.core_options, None);
        let mut expected = sample_meta();
        expected.byte_count = 300;
        assert_eq!(meta, expected);
        // And this build writes the very same bytes when the options are unknown, so nothing
        // about an export without options changed.
        assert_eq!(
            pack_state_export(&sample_meta(), &payload),
            old_format_export(&payload)
        );
    }

    #[test]
    fn known_but_empty_options_stay_distinct_from_unknown() {
        let mut meta = sample_meta();
        meta.core_options = Some(BTreeMap::new());
        let (back, _) = unpack_state_export(&pack_state_export(&meta, &[1])).unwrap();
        assert_eq!(back.core_options, Some(BTreeMap::new()));
        meta.core_options = None;
        let (back, _) = unpack_state_export(&pack_state_export(&meta, &[1])).unwrap();
        assert_eq!(back.core_options, None);
    }

    #[test]
    fn options_are_written_sorted_and_the_output_is_deterministic() {
        let mut meta = sample_meta();
        meta.label = String::new();
        // Given out of order: the file is sorted by key whatever order the host's map was in.
        meta.core_options = Some(options(&[("zeta", "1"), ("a=1", "x\ny")]));
        let first = pack_state_export(&meta, &[1, 2]);
        assert_eq!(first, pack_state_export(&meta.clone(), &[1, 2]));
        assert_eq!(
            header_of(&first),
            "game_id=Super Mario World (USA).sfc\ncore_id=snes9x\ncore_version=1.62.3 abc123\n\
             byte_count=2\nframe=12345\ncreated_at=1700000000.5\nslot=7\nlabel=\n\
             core_option_count=2\ncore_option=a\\=1=x\\ny\ncore_option=zeta=1\n"
        );
    }

    #[test]
    fn an_older_reader_sees_only_unknown_keys_for_the_options() {
        // An older build splits each line at its first `=` and ignores keys it does not know.
        // Escaping keeps every option on one line, so it can never land on a key it does know.
        let mut meta = sample_meta();
        meta.core_options = Some(options(&[
            ("game_id", "Other.sfc"),
            ("k\nbyte_count", "9\nslot=99"),
        ]));
        let bytes = pack_state_export(&meta, &[1, 2, 3]);
        let keys: Vec<&str> = header_of(&bytes)
            .lines()
            .map(|l| l.split_once('=').unwrap().0)
            .collect();
        assert_eq!(
            keys,
            [
                "game_id",
                "core_id",
                "core_version",
                "byte_count",
                "frame",
                "created_at",
                "slot",
                "label",
                "core_option_count",
                "core_option",
                "core_option",
            ]
        );
    }

    #[test]
    fn a_damaged_options_section_degrades_to_unknown_and_still_imports() {
        let payload = [5u8; 10];
        let with_section = |section: &str| {
            let mut bytes = old_format_export(&payload);
            let header = format!("{}{section}", header_of(&bytes));
            bytes.truncate(24);
            bytes[20..24].copy_from_slice(&(header.len() as u32).to_le_bytes());
            bytes.extend_from_slice(header.as_bytes());
            bytes.extend_from_slice(&payload);
            bytes
        };
        for section in [
            // Count says more than there are.
            "core_option_count=2\ncore_option=a=1\n",
            // Count says fewer.
            "core_option_count=0\ncore_option=a=1\n",
            // A line with no separator.
            "core_option_count=1\ncore_option=no separator\\=here\n",
            // The same key twice.
            "core_option_count=2\ncore_option=a=1\ncore_option=a=2\n",
            // A count that is not a number.
            "core_option_count=lots\ncore_option=a=1\n",
            // Option lines with no count at all.
            "core_option=a=1\n",
        ] {
            let (meta, back) = unpack_state_export(&with_section(section)).unwrap();
            assert_eq!(meta.core_options, None, "{section:?}");
            assert_eq!(back, payload);
        }
        let (meta, _) =
            unpack_state_export(&with_section("core_option_count=1\ncore_option=a=1\n")).unwrap();
        assert_eq!(meta.core_options, Some(options(&[("a", "1")])));
    }

    #[test]
    fn options_too_big_for_a_header_are_left_out_rather_than_breaking_the_file() {
        let mut meta = sample_meta();
        let huge = "v".repeat(MAX_HEADER);
        meta.core_options = Some(options(&[("big", &huge)]));
        let bytes = pack_state_export(&meta, &[1, 2, 3]);
        let (back, payload) = unpack_state_export(&bytes).unwrap();
        assert_eq!(payload, [1, 2, 3]);
        assert_eq!(back.core_options, None);
    }

    #[test]
    fn option_escaping_splits_at_the_right_equals_sign() {
        for key in ["", "=", "a=b", "\\", "\\=", "x\\\\=y", "\n=\r", "ü="] {
            let raw = format!("{}={}", escape_option_key(key), escape("v=w"));
            let (k, v) = split_option(&raw).unwrap();
            assert_eq!(
                (unescape(k), unescape(v)),
                (key.to_string(), "v=w".to_string())
            );
        }
        assert_eq!(split_option("no separator"), None);
        assert_eq!(split_option("escaped\\=only"), None);
    }

    #[test]
    fn export_round_trips_every_field() {
        let payload: Vec<u8> = (0..=255).cycle().take(5000).collect();
        let bytes = pack_state_export(&sample_meta(), &payload);
        assert_eq!(&bytes[..16], EXPORT_MAGIC);
        let (meta, back) = unpack_state_export(&bytes).unwrap();
        assert_eq!(back, payload);
        let mut expected = sample_meta();
        expected.byte_count = 5000;
        assert_eq!(meta, expected);
    }

    #[test]
    fn export_without_core_facts_round_trips_as_unknown() {
        let mut meta = sample_meta();
        meta.core_id = None;
        meta.core_version = None;
        let (back, _) = unpack_state_export(&pack_state_export(&meta, &[1, 2, 3])).unwrap();
        assert_eq!(back.core_id, None);
        assert_eq!(back.core_version, None);
    }

    #[test]
    fn import_refuses_a_foreign_file() {
        assert!(unpack_state_export(b"").is_err());
        assert!(unpack_state_export(b"RASTATE\x01\x00\x00\x00 a retroarch state").is_err());
        assert!(unpack_state_export(&[0u8; 100]).is_err());
    }

    #[test]
    fn import_refuses_a_truncated_or_padded_payload() {
        let bytes = pack_state_export(&sample_meta(), &[9u8; 100]);
        let err = unpack_state_export(&bytes[..bytes.len() - 1]).unwrap_err();
        assert!(err.contains("cut short"), "{err}");
        let mut padded = bytes.clone();
        padded.push(0);
        assert!(unpack_state_export(&padded).is_err());
        // Cut inside the header.
        assert!(unpack_state_export(&bytes[..30]).is_err());
    }

    #[test]
    fn import_refuses_a_future_version_and_names_it() {
        let mut bytes = pack_state_export(&sample_meta(), &[1]);
        bytes[16] = 3;
        let err = unpack_state_export(&bytes).unwrap_err();
        assert!(err.contains("version 3"), "{err}");
    }

    #[test]
    fn a_picture_travels_with_the_state() {
        let payload = [3u8; 500];
        let picture = b"\x89PNG not really, but bytes".to_vec();
        let bytes = pack_state_export_with_picture(&sample_meta(), &payload, Some(&picture));
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 2);
        assert!(header_of(&bytes).ends_with(&format!("picture_bytes={}\n", picture.len())));
        let back = unpack_state_export_with_picture(&bytes).unwrap();
        assert_eq!(back.payload, payload);
        assert_eq!(back.picture, Some(picture.clone()));
        let mut expected = sample_meta();
        expected.byte_count = 500;
        assert_eq!(back.meta, expected);
        // The plain reader takes the state and leaves the picture.
        let (_, plain) = unpack_state_export(&bytes).unwrap();
        assert_eq!(plain, payload);
    }

    #[test]
    fn no_picture_writes_the_same_version_1_file_as_before() {
        let payload = [7u8; 300];
        for none in [None, Some(&[][..]), Some(&vec![0u8; MAX_EXPORT_PICTURE + 1][..])] {
            assert_eq!(
                pack_state_export_with_picture(&sample_meta(), &payload, none),
                old_format_export(&payload)
            );
        }
        assert_eq!(unpack_state_export_with_picture(&old_format_export(&payload)).unwrap().picture, None);
    }

    #[test]
    fn a_damaged_picture_costs_only_the_picture() {
        let payload = [9u8; 64];
        let picture = vec![1u8; 40];
        let bytes = pack_state_export_with_picture(&sample_meta(), &payload, Some(&picture));
        // Cut inside the picture: the state is whole, so it imports without one.
        let cut = unpack_state_export_with_picture(&bytes[..bytes.len() - 5]).unwrap();
        assert_eq!(cut.payload, payload);
        assert_eq!(cut.picture, None);
        // Cut inside the STATE: refused, as before.
        let short = &bytes[..bytes.len() - picture.len() - 1];
        assert!(unpack_state_export_with_picture(short).unwrap_err().contains("cut short"));
    }

    #[test]
    fn escaping_round_trips_backslashes() {
        for text in ["a\\b", "\\n literally", "end\\", "\r\n", ""] {
            assert_eq!(unescape(&escape(text)), text);
        }
    }
}
