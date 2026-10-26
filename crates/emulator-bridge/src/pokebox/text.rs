//! The games' own letters. Red to Crystal share one table (the English and European cartridges;
//! Bulbapedia's "Character encoding (Generation I)" and "(Generation II)"), Ruby to LeafGreen
//! another ("Character encoding (Generation III)"). Text moves between them through ordinary
//! letters; a letter one game has and the other lacks becomes "?".

/// Generation 1 and 2: one byte to its letters (some bytes are two letters, like 'd).
fn gb_char(b: u8) -> Option<&'static str> {
    const UPPER: [&str; 26] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"];
    const LOWER: [&str; 26] = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z"];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    Some(match b {
        0x7F => " ",
        0x80..=0x99 => UPPER[usize::from(b - 0x80)],
        0x9A => "(",
        0x9B => ")",
        0x9C => ":",
        0x9D => ";",
        0x9E => "[",
        0x9F => "]",
        0xA0..=0xB9 => LOWER[usize::from(b - 0xA0)],
        0xBA => "é",
        0xBB => "'d",
        0xBC => "'l",
        0xBD => "'s",
        0xBE => "'t",
        0xBF => "'v",
        0xE0 => "'",
        0xE3 => "-",
        0xE4 => "'r",
        0xE5 => "'m",
        0xE6 => "?",
        0xE7 => "!",
        0xE8 => ".",
        0xEF => "♂",
        0xF1 => "×",
        0xF2 => ".",
        0xF3 => "/",
        0xF4 => ",",
        0xF5 => "♀",
        0xF6..=0xFF => DIGITS[usize::from(b - 0xF6)],
        _ => return None,
    })
}

fn gb_byte(c: char) -> Option<u8> {
    Some(match c {
        ' ' => 0x7F,
        'A'..='Z' => 0x80 + (c as u8 - b'A'),
        'a'..='z' => 0xA0 + (c as u8 - b'a'),
        '0'..='9' => 0xF6 + (c as u8 - b'0'),
        '(' => 0x9A,
        ')' => 0x9B,
        ':' => 0x9C,
        ';' => 0x9D,
        '[' => 0x9E,
        ']' => 0x9F,
        'é' => 0xBA,
        '\'' | '’' | '‘' => 0xE0,
        '-' => 0xE3,
        '?' => 0xE6,
        '!' => 0xE7,
        '.' => 0xE8,
        '♂' => 0xEF,
        '×' => 0xF1,
        '/' => 0xF3,
        ',' => 0xF4,
        '♀' => 0xF5,
        _ => return None,
    })
}

fn gba_char(b: u8) -> Option<char> {
    Some(match b {
        0x00 => ' ',
        0x1B => 'é',
        0x5C => '(',
        0x5D => ')',
        0xA1..=0xAA => (b'0' + (b - 0xA1)) as char,
        0xAB => '!',
        0xAC => '?',
        0xAD => '.',
        0xAE => '-',
        0xB0 => '…',
        0xB1 => '“',
        0xB2 => '”',
        0xB3 => '‘',
        0xB4 => '’',
        0xB5 => '♂',
        0xB6 => '♀',
        0xB8 => ',',
        0xB9 => '×',
        0xBA => '/',
        0xBB..=0xD4 => (b'A' + (b - 0xBB)) as char,
        0xD5..=0xEE => (b'a' + (b - 0xD5)) as char,
        0xF0 => ':',
        _ => return None,
    })
}

fn gba_byte(c: char) -> Option<u8> {
    Some(match c {
        ' ' => 0x00,
        'é' => 0x1B,
        '(' => 0x5C,
        ')' => 0x5D,
        '0'..='9' => 0xA1 + (c as u8 - b'0'),
        '!' => 0xAB,
        '?' => 0xAC,
        '.' => 0xAD,
        '-' => 0xAE,
        '…' => 0xB0,
        '“' | '"' => 0xB1,
        '”' => 0xB2,
        '‘' => 0xB3,
        '’' | '\'' => 0xB4,
        '♂' => 0xB5,
        '♀' => 0xB6,
        ',' => 0xB8,
        '×' => 0xB9,
        '/' => 0xBA,
        'A'..='Z' => 0xBB + (c as u8 - b'A'),
        'a'..='z' => 0xD5 + (c as u8 - b'a'),
        ':' => 0xF0,
        _ => return None,
    })
}

/// A name from Red to Crystal: up to the 0x50 terminator.
pub fn gb_decode(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if b == 0x50 {
            break;
        }
        s.push_str(gb_char(b).unwrap_or("?"));
    }
    s
}

/// A name for Red to Crystal, `len` bytes: at most `len - 1` letters, then 0x50 to the end.
/// Says whether every letter could be written.
pub fn gb_encode(text: &str, len: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(len);
    let mut exact = true;
    for c in text.chars() {
        if out.len() + 1 >= len {
            exact = false;
            break;
        }
        match gb_byte(c) {
            Some(b) => out.push(b),
            None => {
                exact = false;
                out.push(0xE6);
            }
        }
    }
    out.resize(len, 0x50);
    (out, exact)
}

/// A name from Ruby to LeafGreen: up to the 0xFF terminator.
pub fn gba_decode(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if b == 0xFF {
            break;
        }
        s.push(gba_char(b).unwrap_or('?'));
    }
    s
}

/// A name for Ruby to LeafGreen in `len` bytes: up to `len` letters, then 0xFF and 0x00 after it.
pub fn gba_encode(text: &str, len: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(len);
    let mut exact = true;
    for c in text.chars() {
        if out.len() >= len {
            exact = false;
            break;
        }
        match gba_byte(c) {
            Some(b) => out.push(b),
            None => {
                exact = false;
                out.push(0xAC);
            }
        }
    }
    if out.len() < len {
        out.push(0xFF);
    }
    out.resize(len, 0x00);
    (out, exact)
}

/// The name a Pokémon has until it is given another, as each generation spells it: capitals,
/// and Mr. Mime without the space before Generation 3.
pub fn default_name(species: u16, gen: u8) -> String {
    let name = super::data::SPECIES.get(usize::from(species)).map_or("", |s| s.name);
    let mut up = name.to_uppercase();
    if gen < 3 {
        up = up.replace(". ", ".").replace('’', "'");
    }
    up
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_go_through_both_tables_and_back() {
        let (b, exact) = gb_encode("PIKACHU", 11);
        assert!(exact);
        assert_eq!(b[..8], [0x8F, 0x88, 0x8A, 0x80, 0x82, 0x87, 0x94, 0x50]);
        assert_eq!(gb_decode(&b), "PIKACHU");
        let (b, _) = gba_encode("Sparky 2", 10);
        assert_eq!(gba_decode(&b), "Sparky 2");
        assert_eq!(b.len(), 10);
        let (b, _) = gba_encode("ABCDEFGHIJ", 10);
        assert!(!b.contains(&0xFF), "ten letters fill it with no end mark");
        let (_, exact) = gb_encode("é…", 11);
        assert!(!exact, "a letter Red has no place for is reported");
        assert_eq!(default_name(122, 1), "MR.MIME");
        assert_eq!(default_name(122, 3), "MR. MIME");
        assert_eq!(default_name(83, 2), "FARFETCH'D");
        assert_eq!(gb_decode(&gb_encode(&default_name(29, 1), 11).0), "NIDORAN♀");
    }
}
