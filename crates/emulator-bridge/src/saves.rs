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
//! The compatibility checks themselves (core id, core version, byte length) stay in the host
//! store, where the live engine is: this module only guarantees the facts survive the round trip.

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
}

/// The first bytes of every export. Sixteen bytes, ending in a newline so `head -c` shows it.
pub const EXPORT_MAGIC: &[u8; 16] = b"CONTINUUM-STATE\n";
const EXPORT_VERSION: u32 = 1;
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
pub fn pack_state_export(meta: &StateExportMeta, payload: &[u8]) -> Vec<u8> {
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

    let mut out = Vec::with_capacity(EXPORT_MAGIC.len() + 8 + header.len() + payload.len());
    out.extend_from_slice(EXPORT_MAGIC);
    out.extend_from_slice(&EXPORT_VERSION.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(payload);
    out
}

/// Reads a file written by [`pack_state_export`]. Every failure is a sentence for the status line.
pub fn unpack_state_export(bytes: &[u8]) -> Result<(StateExportMeta, Vec<u8>), String> {
    if bytes.len() < EXPORT_MAGIC.len() + 8 || &bytes[..EXPORT_MAGIC.len()] != EXPORT_MAGIC {
        return Err(
            "that file is not a Continuum save state export (it does not start with the \
             Continuum header)"
                .into(),
        );
    }
    let at = EXPORT_MAGIC.len();
    let version = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    if version != EXPORT_VERSION {
        return Err(format!(
            "that export is format version {version}, and this build reads version \
             {EXPORT_VERSION}; update the app"
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
    let payload = &bytes[body + header_len..];

    let mut meta = StateExportMeta::default();
    let mut byte_count: Option<u64> = None;
    for line in header.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
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
            // Unknown keys are a later build's additions, and are ignored rather than refused.
            _ => {}
        }
    }
    if meta.game_id.is_empty() {
        return Err("that export does not say which game it belongs to".into());
    }
    let Some(expected) = byte_count else {
        return Err("that export does not record its own length, so it cannot be checked".into());
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
    Ok((meta, payload.to_vec()))
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
        }
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
        bytes[16] = 2;
        let err = unpack_state_export(&bytes).unwrap_err();
        assert!(err.contains("version 2"), "{err}");
    }

    #[test]
    fn escaping_round_trips_backslashes() {
        for text in ["a\\b", "\\n literally", "end\\", "\r\n", ""] {
            assert_eq!(unescape(&escape(text)), text);
        }
    }
}
