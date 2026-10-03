//! A tiny JSON reader and writer for FLAT objects: `{"key": value, ...}` where every value is a
//! string, a number, a boolean or null. That is the whole shape of the two save formats the
//! bundled players use (Manic's Flash `.json` and the metadata files inside a `.J2meJS.srm`), so
//! a full JSON library would be a dependency for nothing.
//!
//! Anything else (a nested object, an array, trailing junk) is an error with a plain reason,
//! never a guess.

/// One value of a flat object.
#[derive(Debug, Clone, PartialEq)]
pub enum FlatValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

impl FlatValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FlatValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FlatValue::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            FlatValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while let Some(&(_, c)) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        self.skip_ws();
        match self.chars.next() {
            Some((_, c)) if c == want => Ok(()),
            Some((at, c)) => Err(format!("expected '{want}' at byte {at}, found '{c}'")),
            None => Err(format!("expected '{want}', found the end of the text")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let Some((at, c)) = self.chars.next() else {
                return Err("a string was never closed".into());
            };
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some((_, e)) = self.chars.next() else {
                        return Err("a string ends inside an escape".into());
                    };
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let high = self.hex4()?;
                            if (0xD800..0xDC00).contains(&high) {
                                // A surrogate pair: the low half must follow as another \u.
                                self.expect('\\')?;
                                match self.chars.next() {
                                    Some((_, 'u')) => {}
                                    _ => return Err("a lone high surrogate in a \\u escape".into()),
                                }
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err("a high surrogate not followed by a low one".into());
                                }
                                let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                                out.push(char::from_u32(code).ok_or("a bad surrogate pair")?);
                            } else {
                                out.push(char::from_u32(high).ok_or("a lone low surrogate")?);
                            }
                        }
                        other => return Err(format!("unknown escape \\{other} at byte {at}")),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(format!("a raw control character inside a string at byte {at}"))
                }
                c => out.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some((_, c)) = self.chars.next() else {
                return Err("a \\u escape is cut short".into());
            };
            value = value * 16 + c.to_digit(16).ok_or("a \\u escape has a non-hex digit")?;
        }
        Ok(value)
    }

    fn word(&mut self) -> String {
        let mut out = String::new();
        while let Some(&(_, c)) = self.chars.peek() {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.') {
                out.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        out
    }

    fn value(&mut self) -> Result<FlatValue, String> {
        self.skip_ws();
        match self.chars.peek().map(|&(_, c)| c) {
            Some('"') => Ok(FlatValue::Str(self.string()?)),
            Some('{') | Some('[') => Err("a nested object or array; only flat objects are read".into()),
            Some(_) => {
                let word = self.word();
                match word.as_str() {
                    "true" => Ok(FlatValue::Bool(true)),
                    "false" => Ok(FlatValue::Bool(false)),
                    "null" => Ok(FlatValue::Null),
                    "" => Err("a value is missing".into()),
                    number => number
                        .parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite())
                        .map(FlatValue::Num)
                        .ok_or_else(|| format!("'{number}' is not a JSON value")),
                }
            }
            None => Err("the text ends where a value should be".into()),
        }
    }
}

/// Reads a flat JSON object, keeping the keys in the order they appear.
pub fn parse_object(text: &str) -> Result<Vec<(String, FlatValue)>, String> {
    // A byte order mark is not JSON, but files saved by some editors start with one.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut r = Reader { chars: text.char_indices().peekable() };
    r.expect('{')?;
    let mut out = Vec::new();
    r.skip_ws();
    if let Some(&(_, '}')) = r.chars.peek() {
        r.chars.next();
    } else {
        loop {
            let key = r.string()?;
            r.expect(':')?;
            let value = r.value()?;
            out.push((key, value));
            r.skip_ws();
            match r.chars.next() {
                Some((_, ',')) => continue,
                Some((_, '}')) => break,
                Some((at, c)) => return Err(format!("expected ',' or '}}' at byte {at}, found '{c}'")),
                None => return Err("the object is never closed".into()),
            }
        }
    }
    r.skip_ws();
    if let Some((at, _)) = r.chars.next() {
        return Err(format!("text after the object at byte {at}"));
    }
    Ok(out)
}

/// A JSON string literal, quotes included.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number the way JavaScript's JSON.stringify writes it: integers without a fraction.
pub fn number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9.0e15 {
        format!("{}", n as i64)
    } else if n.is_finite() {
        format!("{n}")
    } else {
        "null".into()
    }
}

/// Writes a flat object, in the order given.
pub fn write_object(entries: &[(String, FlatValue)]) -> String {
    let mut out = String::from("{");
    for (i, (key, value)) in entries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&quote(key));
        out.push(':');
        match value {
            FlatValue::Str(s) => out.push_str(&quote(s)),
            FlatValue::Num(n) => out.push_str(&number(*n)),
            FlatValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            FlatValue::Null => out.push_str("null"),
        }
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_value_kind_in_order() {
        let got = parse_object(r#" { "b": "x\"y\\z\n", "a": 12, "c": true, "d": null, "e": -1.5e2 } "#)
            .unwrap();
        assert_eq!(
            got,
            vec![
                ("b".into(), FlatValue::Str("x\"y\\z\n".into())),
                ("a".into(), FlatValue::Num(12.0)),
                ("c".into(), FlatValue::Bool(true)),
                ("d".into(), FlatValue::Null),
                ("e".into(), FlatValue::Num(-150.0)),
            ]
        );
    }

    #[test]
    fn unicode_escapes_and_surrogates() {
        let got = parse_object(r#"{"k":"\u00e9\ud83d\ude00/"}"#).unwrap();
        assert_eq!(got[0].1, FlatValue::Str("\u{e9}\u{1f600}/".into()));
    }

    #[test]
    fn refuses_what_it_does_not_read() {
        assert!(parse_object(r#"{"a":{"b":1}}"#).is_err());
        assert!(parse_object(r#"{"a":[1]}"#).is_err());
        assert!(parse_object(r#"{"a":1} junk"#).is_err());
        assert!(parse_object(r#"{"a":1"#).is_err());
        assert!(parse_object(r#"["a"]"#).is_err());
        assert!(parse_object(r#"{"a":nope}"#).is_err());
        assert!(parse_object("").is_err());
    }

    #[test]
    fn empty_object_and_bom() {
        assert!(parse_object("\u{feff}{ }").unwrap().is_empty());
    }

    #[test]
    fn writes_what_it_reads() {
        let entries = vec![
            ("x/y".to_string(), FlatValue::Str("a\"b\u{1}".into())),
            ("n".to_string(), FlatValue::Num(1700000000000.0)),
            ("f".to_string(), FlatValue::Num(0.5)),
            ("t".to_string(), FlatValue::Bool(false)),
        ];
        let text = write_object(&entries);
        assert_eq!(text, r#"{"x/y":"a\"b\u0001","n":1700000000000,"f":0.5,"t":false}"#);
        assert_eq!(parse_object(&text).unwrap(), entries);
    }
}
