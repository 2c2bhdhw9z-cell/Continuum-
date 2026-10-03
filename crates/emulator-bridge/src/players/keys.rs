//! What each pad button sends to a bundled player: a keyboard key for Flash (Ruffle reads
//! `KeyboardEvent.code`, `key` and `keyCode`), a phone key for J2ME (a MIDP key code).
//!
//! Pad buttons are named by their W3C standard-gamepad index, the same numbers `PadSlot` uses in
//! TouchControls.swift and the engine's gamepad layer uses: 0 south (`b`), 1 east (`a`), 2 west
//! (`y`), 3 north (`x`), 4 `l`, 5 `r`, 6 `l2`, 7 `r2`, 8 select, 9 start, 10 `l3`, 11 `r3`,
//! 12 up, 13 down, 14 left, 15 right.
//!
//! A per-game remap is stored as the DIFFERENCES from the defaults, written `slot=token;...`
//! (`none` unbinds a button), so a game with no remap stores nothing and a later change to a
//! default reaches every game that never changed that button.

/// The two players, by their shared system ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Player {
    Flash,
    J2me,
}

impl Player {
    pub fn from_system(system: &str) -> Option<Player> {
        match system {
            "flash" => Some(Player::Flash),
            "j2me" => Some(Player::J2me),
            _ => None,
        }
    }
}

/// The number of pad buttons, the W3C standard gamepad's sixteen.
pub const SLOT_COUNT: u8 = 16;

/// A keyboard key as a browser describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashKey {
    /// `KeyboardEvent.code`, which is also the token stored in a remap.
    pub code: &'static str,
    /// `KeyboardEvent.key`.
    pub key: &'static str,
    /// The legacy `keyCode`, which older AVM1 games read through Key.getCode().
    pub key_code: u32,
    /// What the settings screen shows.
    pub label: &'static str,
}

const fn fk(code: &'static str, key: &'static str, key_code: u32, label: &'static str) -> FlashKey {
    FlashKey { code, key, key_code, label }
}

/// Every key a Flash pad button can send. Letters and digits are generated below.
const FLASH_NAMED: &[FlashKey] = &[
    fk("ArrowUp", "ArrowUp", 38, "Up arrow"),
    fk("ArrowDown", "ArrowDown", 40, "Down arrow"),
    fk("ArrowLeft", "ArrowLeft", 37, "Left arrow"),
    fk("ArrowRight", "ArrowRight", 39, "Right arrow"),
    fk("Space", " ", 32, "Space"),
    fk("Enter", "Enter", 13, "Enter"),
    fk("Escape", "Escape", 27, "Esc"),
    fk("Tab", "Tab", 9, "Tab"),
    fk("Backspace", "Backspace", 8, "Backspace"),
    fk("ShiftLeft", "Shift", 16, "Shift"),
    fk("ControlLeft", "Control", 17, "Ctrl"),
    fk("AltLeft", "Alt", 18, "Alt"),
];

const LETTERS: &[(&str, &str, &str)] = &[
    ("KeyA", "a", "A"), ("KeyB", "b", "B"), ("KeyC", "c", "C"), ("KeyD", "d", "D"),
    ("KeyE", "e", "E"), ("KeyF", "f", "F"), ("KeyG", "g", "G"), ("KeyH", "h", "H"),
    ("KeyI", "i", "I"), ("KeyJ", "j", "J"), ("KeyK", "k", "K"), ("KeyL", "l", "L"),
    ("KeyM", "m", "M"), ("KeyN", "n", "N"), ("KeyO", "o", "O"), ("KeyP", "p", "P"),
    ("KeyQ", "q", "Q"), ("KeyR", "r", "R"), ("KeyS", "s", "S"), ("KeyT", "t", "T"),
    ("KeyU", "u", "U"), ("KeyV", "v", "V"), ("KeyW", "w", "W"), ("KeyX", "x", "X"),
    ("KeyY", "y", "Y"), ("KeyZ", "z", "Z"),
];

const DIGITS: &[(&str, &str)] = &[
    ("Digit0", "0"), ("Digit1", "1"), ("Digit2", "2"), ("Digit3", "3"), ("Digit4", "4"),
    ("Digit5", "5"), ("Digit6", "6"), ("Digit7", "7"), ("Digit8", "8"), ("Digit9", "9"),
];

/// Every Flash key, in the order the settings screen lists them.
pub fn flash_keys() -> Vec<FlashKey> {
    let mut out: Vec<FlashKey> = FLASH_NAMED.to_vec();
    for (i, (code, key, label)) in LETTERS.iter().enumerate() {
        out.push(fk(code, key, 65 + i as u32, label));
    }
    for (i, (code, key)) in DIGITS.iter().enumerate() {
        out.push(fk(code, key, 48 + i as u32, key));
    }
    out
}

/// The Flash key for a token, or None.
pub fn flash_key(token: &str) -> Option<FlashKey> {
    flash_keys().into_iter().find(|k| k.code == token)
}

/// A phone key. The code depends on the phone type for the five keys that differ between a
/// Nokia-style phone and a plain MIDP one; see [`j2me_key_code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct J2meKey {
    pub token: &'static str,
    pub label: &'static str,
}

const fn jk(token: &'static str, label: &'static str) -> J2meKey {
    J2meKey { token, label }
}

/// Every phone key, in the order the settings screen lists them.
pub const J2ME_KEYS: &[J2meKey] = &[
    jk("UP", "Up"), jk("DOWN", "Down"), jk("LEFT", "Left"), jk("RIGHT", "Right"),
    jk("OK", "OK (fire)"), jk("LSK", "Left soft key"), jk("RSK", "Right soft key"),
    jk("CLEAR", "Clear"),
    jk("NUM0", "0"), jk("NUM1", "1"), jk("NUM2", "2"), jk("NUM3", "3"), jk("NUM4", "4"),
    jk("NUM5", "5"), jk("NUM6", "6"), jk("NUM7", "7"), jk("NUM8", "8"), jk("NUM9", "9"),
    jk("STAR", "*"), jk("POUND", "#"),
];

/// How the phone reports its navigation keys.
///
/// `nokia` is what most games of the era were written against and what the J2ME engine's own
/// keyboard handler sends: the D-pad and OK are negative codes (-1 up, -2 down, -3 left, -4 right,
/// -5 fire), the soft keys -6 and -7, Clear -8. `standard` is the plain MIDP phone with no
/// dedicated navigation keys, where the D-pad IS the 2, 8, 4 and 6 keys and OK is 5; some games
/// only respond to that. The digits, * and # are the same on both (their ASCII codes, MIDP's
/// KEY_NUM0..KEY_NUM9, KEY_STAR 42 and KEY_POUND 35).
pub const PHONE_TYPES: &[&str] = &["nokia", "standard"];

/// The MIDP key code a phone key sends, or None for an unknown token or phone type.
pub fn j2me_key_code(token: &str, phone_type: &str) -> Option<i32> {
    let standard = match phone_type {
        "nokia" => false,
        "standard" => true,
        _ => return None,
    };
    let code = match token {
        "UP" => if standard { 50 } else { -1 },
        "DOWN" => if standard { 56 } else { -2 },
        "LEFT" => if standard { 52 } else { -3 },
        "RIGHT" => if standard { 54 } else { -4 },
        "OK" => if standard { 53 } else { -5 },
        "LSK" => -6,
        "RSK" => -7,
        "CLEAR" => -8,
        "STAR" => 42,
        "POUND" => 35,
        t => {
            let digit = t.strip_prefix("NUM")?;
            let n: i32 = digit.parse().ok().filter(|n| (0..=9).contains(n) && digit.len() == 1)?;
            48 + n
        }
    };
    Some(code)
}

/// The default binding of every pad button, by slot.
pub fn defaults(player: Player) -> Vec<(u8, &'static str)> {
    match player {
        // Arrows, Space, Z and X, Enter, as Flash games of the time almost all used.
        Player::Flash => vec![
            (12, "ArrowUp"), (13, "ArrowDown"), (14, "ArrowLeft"), (15, "ArrowRight"),
            (1, "Space"), (0, "KeyZ"), (3, "KeyX"), (2, "KeyC"),
            (9, "Enter"), (8, "Escape"),
            (4, "ShiftLeft"), (5, "ControlLeft"),
            (6, "Digit1"), (7, "Digit2"),
            (10, "KeyQ"), (11, "KeyE"),
        ],
        // The face cluster is the phone keypad around OK (see GameSystem.j2me in
        // TouchControls.swift); the soft keys sit where Select and Start are.
        Player::J2me => vec![
            (12, "UP"), (13, "DOWN"), (14, "LEFT"), (15, "RIGHT"),
            (1, "OK"), (0, "NUM0"), (2, "NUM1"), (3, "NUM3"),
            (6, "NUM7"), (7, "NUM9"),
            (4, "STAR"), (5, "POUND"),
            (8, "LSK"), (9, "RSK"),
            (10, "NUM5"), (11, "CLEAR"),
        ],
    }
}

fn token_known(player: Player, token: &str) -> bool {
    match player {
        Player::Flash => flash_key(token).is_some(),
        Player::J2me => J2ME_KEYS.iter().any(|k| k.token == token),
    }
}

/// The token stored for "this button sends nothing".
pub const UNBOUND: &str = "none";

/// The full table for a game: the defaults with its overrides on top. Every slot appears once,
/// in slot order; an unbound slot carries [`UNBOUND`].
pub fn resolve(player: Player, overrides: &str) -> Result<Vec<(u8, String)>, String> {
    let mut table: Vec<(u8, String)> = (0..SLOT_COUNT).map(|s| (s, UNBOUND.to_string())).collect();
    for (slot, token) in defaults(player) {
        table[slot as usize].1 = token.to_string();
    }
    for item in overrides.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (slot, token) = item
            .split_once('=')
            .ok_or_else(|| format!("'{item}' is not slot=key"))?;
        let slot: u8 = slot
            .trim()
            .parse()
            .ok()
            .filter(|s| *s < SLOT_COUNT)
            .ok_or_else(|| format!("'{slot}' is not a pad button number 0 to 15"))?;
        let token = token.trim();
        if token != UNBOUND && !token_known(player, token) {
            return Err(format!("'{token}' is not a key this player knows"));
        }
        table[slot as usize].1 = token.to_string();
    }
    Ok(table)
}

/// The overrides string for a full table: only the slots that differ from the defaults.
pub fn encode_overrides(player: Player, table: &[(u8, String)]) -> Result<String, String> {
    let base = resolve(player, "")?;
    let mut parts = Vec::new();
    let mut seen = [false; SLOT_COUNT as usize];
    for (slot, token) in table {
        if *slot >= SLOT_COUNT {
            return Err(format!("{slot} is not a pad button number 0 to 15"));
        }
        if seen[*slot as usize] {
            return Err(format!("pad button {slot} is listed twice"));
        }
        seen[*slot as usize] = true;
        if token != UNBOUND && !token_known(player, token) {
            return Err(format!("'{token}' is not a key this player knows"));
        }
        if base[*slot as usize].1 != *token {
            parts.push(format!("{slot}={token}"));
        }
    }
    Ok(parts.join(";"))
}

/// The screen sizes the J2ME settings offer, the common phone resolutions of the era.
pub const J2ME_SCREEN_SIZES: &[&str] = &[
    "128x128", "128x160", "176x208", "176x220", "208x208", "240x320", "320x240", "352x416",
    "360x640", "640x360",
];

/// The size a new game starts at: the most common one, and the engine's own default.
pub const J2ME_DEFAULT_SCREEN: (u32, u32) = (240, 320);

/// "240x320" (also `240*320`, `240,320`) as a width and height, each 64 to 1024.
pub fn parse_screen_size(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    let (w, h) = text.split_once(['x', 'X', '*', ','])?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    let ok = |v: u32| (64..=1024).contains(&v);
    (ok(w) && ok(h)).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_defaults_are_the_promised_keys() {
        let table = resolve(Player::Flash, "").unwrap();
        assert_eq!(table.len(), 16);
        assert_eq!(table[12].1, "ArrowUp");
        assert_eq!(table[15].1, "ArrowRight");
        assert_eq!(table[1].1, "Space");
        assert_eq!(table[0].1, "KeyZ");
        assert_eq!(table[3].1, "KeyX");
        assert_eq!(table[9].1, "Enter");
        for (_, token) in &table {
            assert!(token == UNBOUND || flash_key(token).is_some(), "{token} is in the catalog");
        }
    }

    #[test]
    fn flash_catalog_codes_are_browser_codes() {
        let space = flash_key("Space").unwrap();
        assert_eq!((space.key, space.key_code), (" ", 32));
        let z = flash_key("KeyZ").unwrap();
        assert_eq!((z.key, z.key_code), ("z", 90));
        let a = flash_key("KeyA").unwrap();
        assert_eq!(a.key_code, 65);
        assert_eq!(flash_key("Digit7").unwrap().key_code, 55);
        assert_eq!(flash_key("ArrowLeft").unwrap().key_code, 37);
        assert!(flash_key("KeyAA").is_none());
        let keys = flash_keys();
        let mut codes: Vec<_> = keys.iter().map(|k| k.code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), keys.len(), "no code twice");
    }

    #[test]
    fn j2me_codes_follow_the_phone_type() {
        assert_eq!(j2me_key_code("UP", "nokia"), Some(-1));
        assert_eq!(j2me_key_code("RIGHT", "nokia"), Some(-4));
        assert_eq!(j2me_key_code("OK", "nokia"), Some(-5));
        assert_eq!(j2me_key_code("UP", "standard"), Some(50));
        assert_eq!(j2me_key_code("LEFT", "standard"), Some(52));
        assert_eq!(j2me_key_code("OK", "standard"), Some(53));
        assert_eq!(j2me_key_code("LSK", "standard"), Some(-6));
        assert_eq!(j2me_key_code("RSK", "nokia"), Some(-7));
        assert_eq!(j2me_key_code("NUM0", "nokia"), Some(48));
        assert_eq!(j2me_key_code("NUM9", "standard"), Some(57));
        assert_eq!(j2me_key_code("STAR", "nokia"), Some(42));
        assert_eq!(j2me_key_code("POUND", "nokia"), Some(35));
        assert_eq!(j2me_key_code("NUM10", "nokia"), None);
        assert_eq!(j2me_key_code("NUM", "nokia"), None);
        assert_eq!(j2me_key_code("UP", "siemens"), None);
        for key in J2ME_KEYS {
            for phone in PHONE_TYPES {
                assert!(j2me_key_code(key.token, phone).is_some(), "{} on {phone}", key.token);
            }
        }
    }

    #[test]
    fn j2me_defaults_cover_the_keypad() {
        let table = resolve(Player::J2me, "").unwrap();
        let tokens: Vec<&str> = table.iter().map(|(_, t)| t.as_str()).collect();
        for want in ["UP", "DOWN", "LEFT", "RIGHT", "OK", "LSK", "RSK", "NUM0", "NUM1", "NUM3",
                     "NUM7", "NUM9", "STAR", "POUND"] {
            assert!(tokens.contains(&want), "{want} is on the pad");
        }
        assert_eq!(table[8].1, "LSK");
        assert_eq!(table[9].1, "RSK");
    }

    #[test]
    fn overrides_round_trip_and_store_only_differences() {
        assert_eq!(encode_overrides(Player::Flash, &resolve(Player::Flash, "").unwrap()).unwrap(), "");
        let table = resolve(Player::Flash, "1=KeyW; 9=none").unwrap();
        assert_eq!(table[1].1, "KeyW");
        assert_eq!(table[9].1, UNBOUND);
        assert_eq!(table[12].1, "ArrowUp");
        let text = encode_overrides(Player::Flash, &table).unwrap();
        assert_eq!(text, "1=KeyW;9=none");
        assert_eq!(resolve(Player::Flash, &text).unwrap(), table);

        let phone = resolve(Player::J2me, "12=NUM2;13=NUM8").unwrap();
        assert_eq!(encode_overrides(Player::J2me, &phone).unwrap(), "12=NUM2;13=NUM8");
    }

    #[test]
    fn bad_overrides_are_refused_with_a_reason() {
        assert!(resolve(Player::Flash, "16=Space").is_err());
        assert!(resolve(Player::Flash, "x=Space").is_err());
        assert!(resolve(Player::Flash, "1=NUM5").is_err());
        assert!(resolve(Player::J2me, "1=Space").is_err());
        assert!(resolve(Player::Flash, "1").is_err());
        let mut twice = resolve(Player::Flash, "").unwrap();
        twice.push((1, "Space".into()));
        assert!(encode_overrides(Player::Flash, &twice).is_err());
    }

    #[test]
    fn screen_sizes_parse() {
        assert_eq!(parse_screen_size("240x320"), Some((240, 320)));
        assert_eq!(parse_screen_size(" 176*208 "), Some((176, 208)));
        assert_eq!(parse_screen_size("640,360"), Some((640, 360)));
        assert_eq!(parse_screen_size("32x32"), None);
        assert_eq!(parse_screen_size("2000x320"), None);
        assert_eq!(parse_screen_size("240"), None);
        assert_eq!(parse_screen_size("axb"), None);
        for size in J2ME_SCREEN_SIZES {
            assert!(parse_screen_size(size).is_some(), "{size}");
        }
        assert!(J2ME_SCREEN_SIZES.contains(&"240x320"));
    }
}
