//! Flash saves: Ruffle's SharedObjects, as Manic EMU's `.json` file.
//!
//! Ruffle in a web page keeps every SharedObject in the page's `localStorage`, one item per
//! object, under the key `<host>/<movie path>/<object name>` (see `get_local` in Ruffle's
//! core/src/avm2/globals/flash/net/shared_object.rs at v0.6.0), with the value a base64 AMF blob.
//! Manic's `.json` is that storage dumped as one flat JSON object of key to value, and so is ours:
//! the player page seeds `localStorage` from it before Ruffle starts and dumps it back after.
//!
//! The KEYS carry the page's host and the movie's file name. Continuum's player page is at
//! `continuum-player://localhost/` and loads the movie as `<file name>` beside it, so a key here
//! reads `localhost/<file name>/<object>`, which is what Manic's localhost page produces for the
//! same file. A save made elsewhere (another host, or the same game under another file name) is
//! re-rooted on import by [`rehost`], so the game finds it.

use super::flat_json::{self, FlatValue};

/// The host every key is stored under on this player page.
pub const PLAYER_HOST: &str = "localhost";

/// Reads a `.json` save into its items. Values must be text, as Ruffle writes them; a `null`
/// value is skipped the way Manic's loader skips it.
pub fn decode(text: &str) -> Result<Vec<(String, String)>, String> {
    let object = flat_json::parse_object(text).map_err(|e| format!("not a Flash save: {e}"))?;
    let mut out = Vec::with_capacity(object.len());
    for (key, value) in object {
        match value {
            FlatValue::Str(s) => out.push((key, s)),
            FlatValue::Null => {}
            _ => return Err(format!("not a Flash save: the value of '{key}' is not text")),
        }
    }
    Ok(out)
}

/// Writes items as a `.json` save.
pub fn encode(items: &[(String, String)]) -> String {
    let entries: Vec<(String, FlatValue)> = items
        .iter()
        .map(|(k, v)| (k.clone(), FlatValue::Str(v.clone())))
        .collect();
    flat_json::write_object(&entries)
}

/// A file name as one URL path segment, the way the `url` crate's `push` writes it (the WHATWG
/// path percent-encode set plus `/` and `%`). Ruffle builds the movie URL that way, so this is
/// the segment its keys contain.
pub fn movie_segment(file_name: &str) -> String {
    let mut out = String::with_capacity(file_name.len());
    for byte in file_name.bytes() {
        let encode = !(0x20..0x7F).contains(&byte)
            || matches!(byte, b' ' | b'"' | b'#' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}' | b'/' | b'%');
        if encode {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push(byte as char);
        }
    }
    out
}

/// Re-roots imported items onto this page and this movie file. Returns the items and how many
/// keys changed. A key that is not `host/...` at all is kept as it is.
pub fn rehost(items: Vec<(String, String)>, swf_file_name: &str) -> (Vec<(String, String)>, usize) {
    let ours = movie_segment(swf_file_name);
    let mut changed = 0;
    let out = items
        .into_iter()
        .map(|(key, value)| {
            let Some((_host, rest)) = key.split_once('/') else {
                return (key, value);
            };
            let mut rest = rest.to_string();
            // The movie path, when the object was keyed by it: everything up to and including the
            // first `.swf` segment (not counting the object's own name, the last segment) becomes
            // this movie's one segment, which is the whole movie path on this page. A key made
            // with a `localPath` that stops above the movie has no `.swf` segment and keeps its
            // path.
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 2 {
                if let Some(at) = parts[..parts.len() - 1]
                    .iter()
                    .position(|p| p.to_ascii_lowercase().ends_with(".swf"))
                {
                    let tail = parts[at + 1..].join("/");
                    rest = format!("{ours}/{tail}");
                }
            }
            let new_key = format!("{PLAYER_HOST}/{rest}");
            if new_key != key {
                changed += 1;
            }
            (new_key, value)
        })
        .collect();
    (out, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manic_json_round_trips() {
        let text = r#"{"localhost/Bloons.swf/save":"CgsBCWJhbmc=","localhost//global":"AQ=="}"#;
        let items = decode(text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "localhost/Bloons.swf/save");
        assert_eq!(encode(&items), text);
    }

    #[test]
    fn non_text_values_are_refused_and_nulls_skipped() {
        assert!(decode(r#"{"a":1}"#).is_err());
        assert!(decode("not json").is_err());
        assert_eq!(decode(r#"{"a":null,"b":"x"}"#).unwrap(), vec![("b".into(), "x".into())]);
        assert!(decode("{}").unwrap().is_empty());
    }

    #[test]
    fn segment_encoding_matches_the_url_crate() {
        assert_eq!(movie_segment("Bloons TD.swf"), "Bloons%20TD.swf");
        assert_eq!(movie_segment("a#b?c%.swf"), "a%23b%3Fc%25.swf");
        assert_eq!(movie_segment("caf\u{e9}.swf"), "caf%C3%A9.swf");
        assert_eq!(movie_segment("plain-name_1.swf"), "plain-name_1.swf");
    }

    #[test]
    fn rehost_moves_other_hosts_and_other_file_names() {
        let items = vec![
            ("localhost/My%20Game.swf/hiscore".to_string(), "A".to_string()),
            ("127.0.0.1/file/abc/old.swf/x".to_string(), "B".to_string()),
            ("example.com/games/Game.SWF/slot1".to_string(), "C".to_string()),
            ("example.com//shared".to_string(), "D".to_string()),
            ("nohost".to_string(), "E".to_string()),
        ];
        let (out, changed) = rehost(items, "My Game.swf");
        assert_eq!(out[0].0, "localhost/My%20Game.swf/hiscore");
        assert_eq!(out[1].0, "localhost/My%20Game.swf/x");
        assert_eq!(out[2].0, "localhost/My%20Game.swf/slot1");
        assert_eq!(out[3].0, "localhost//shared");
        assert_eq!(out[4].0, "nohost");
        assert_eq!(changed, 3);
    }
}
