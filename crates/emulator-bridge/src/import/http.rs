//! The Wi-Fi transfer server's protocol half: request heads in, response heads out, and the page.
//!
//! The socket is the app's (NWListener on iOS). Bodies never pass through here: an upload is a
//! plain `PUT /<code>/upload?name=<file>` whose body the app streams straight to a file, so a
//! 700 MB disc track costs a buffer rather than 700 MB of memory. Only the head (request line and
//! headers, capped at [`MAX_HEAD`]) is parsed in Rust.
//!
//! Access: every request has to start with `/<code>/`, where the code is a fresh random
//! [`ACCESS_CODE_LEN`]-character word drawn by the app each time the server is switched on and
//! shown as part of the address (`http://192.168.1.20:8080/k7m2qx/`). Anything else gets a bare
//! 404, so someone else on the same Wi-Fi can neither upload nor read saves, and is not even told
//! there is something to guess. [`strip_access_code`] is the check; [`transfer_address`] builds the
//! address with the trailing slash the page's relative URLs depend on.

use super::webdav::percent_decode;

/// The largest request head accepted. Browsers send well under 8 KB.
pub const MAX_HEAD: usize = 16 * 1024;

/// The upload page, served at `/<code>/`. Plain HTML and script with no outside resources, so it
/// works on a network with no internet. Its URLs are relative (`api/library`, `upload?name=`,
/// `saves/`), so they stay under the code.
pub const PAGE: &str = include_str!("wifi_page.html");

/// The characters an access code is drawn from: digits and lowercase letters without the ones
/// that are easy to misread or mistype (0 o 1 l i). 31 characters.
pub const ACCESS_CODE_ALPHABET: &str = "23456789abcdefghjkmnpqrstuvwxyz";

/// How many characters an access code has. 31^6 is about 887 million codes, for a server that
/// only runs while someone is looking at the phone.
pub const ACCESS_CODE_LEN: usize = 6;

/// True when `code` is a well-formed access code: exactly [`ACCESS_CODE_LEN`] characters, all from
/// [`ACCESS_CODE_ALPHABET`].
pub fn is_access_code(code: &str) -> bool {
    code.len() == ACCESS_CODE_LEN && code.bytes().all(|b| ACCESS_CODE_ALPHABET.as_bytes().contains(&b))
}

/// Compares two byte strings in time that depends only on their lengths, never on where they
/// first differ, so response timing cannot be used to guess the code a character at a time.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    // Keeps the optimiser from turning the fold back into an early exit.
    std::hint::black_box(diff) == 0
}

/// Checks that `path` starts with `/<code>` and returns what follows it, or None.
///
/// `path` is the decoded path without the query, as in [`RequestHead::path`] (so `%6B7m2qx` has
/// already become `k7m2qx`, and `?` starts no query here). The first segment is compared to the
/// code in constant time, ignoring ASCII case so a capital typed by mistake still works.
///
/// - `/k7m2qx/` gives `Some("/")` and `/k7m2qx/api/library` gives `Some("/api/library")`: route
///   the remainder exactly as an unprotected server would route the whole path.
/// - `/k7m2qx` gives `Some("")`: the code with no slash after it. Redirect to `/k7m2qx/`, or the
///   page's relative URLs resolve against `/` and miss the code.
/// - Anything else, including every path when `code` is not a well-formed code, gives None:
///   answer 404 with no detail.
pub fn strip_access_code(path: &str, code: &str) -> Option<String> {
    if !is_access_code(code) {
        return None;
    }
    let rest = path.strip_prefix('/')?;
    let (segment, remainder) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    if !constant_time_eq(segment.to_ascii_lowercase().as_bytes(), code.as_bytes()) {
        return None;
    }
    Some(remainder.to_string())
}

/// The address to show on the phone: `http://<ip>:<port>/<code>/`, always ending in a slash.
pub fn transfer_address(ip: &str, port: u16, code: &str) -> String {
    format!("http://{ip}:{port}/{code}/")
}

/// A parsed request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    /// The decoded path without the query.
    pub path: String,
    /// Decoded query pairs, in order.
    pub query: Vec<(String, String)>,
    /// Header names lowercased.
    pub headers: Vec<(String, String)>,
    /// Content-Length, 0 when absent.
    pub content_length: u64,
    /// How many bytes of the buffer the head took, including the blank line.
    pub head_len: usize,
}

impl RequestHead {
    pub fn query_value(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        let key = key.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }
}

/// Where the head ends (index just past `\r\n\r\n`), or None when more bytes are needed.
pub fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn decode_query_part(text: &str) -> String {
    percent_decode(&text.replace('+', " "))
}

/// Parses the request head at the start of `buf`.
///
/// `Ok(None)` means the head is not complete yet; `Err` is a sentence for a 400.
pub fn parse_head(buf: &[u8]) -> Result<Option<RequestHead>, String> {
    let Some(end) = head_end(buf) else {
        if buf.len() > MAX_HEAD {
            return Err("the request head is too large".into());
        }
        return Ok(None);
    };
    if end > MAX_HEAD {
        return Err("the request head is too large".into());
    }
    let text = std::str::from_utf8(&buf[..end - 4]).map_err(|_| "the request head is not text".to_string())?;
    let mut lines = text.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let target = parts.next().ok_or("the request line has no target")?;
    if method.is_empty() || !parts.next().unwrap_or("").starts_with("HTTP/") {
        return Err("not an HTTP request".into());
    }
    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let query = raw_query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode_query_part(k), decode_query_part(v))
        })
        .collect();
    let mut headers = Vec::new();
    let mut content_length = 0u64;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().to_string();
            if key == "content-length" {
                content_length = value.parse().map_err(|_| "Content-Length is not a number".to_string())?;
            }
            headers.push((key, value));
        }
    }
    Ok(Some(RequestHead {
        method,
        path: percent_decode(raw_path),
        query,
        headers,
        content_length,
        head_len: end,
    }))
}

/// The bare filename an upload may be stored as, or None. Folders, `..` and control characters
/// are refused, so a crafted name cannot write outside the import folder.
pub fn safe_file_name(name: &str) -> Option<String> {
    let base = name.replace('\\', "/");
    let base = base.rsplit('/').next()?.trim();
    if base.is_empty() || base == "." || base == ".." || base.starts_with('.') || base.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(base.chars().take(255).collect())
}

/// A response head with the given status, type and body length.
pub fn response_head(status: u16, content_type: &str, length: u64, extra: &[(String, String)]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        507 => "Insufficient Storage",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\nCache-Control: no-store\r\n"
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    head.into_bytes()
}

/// Escapes a string for a JSON string literal.
pub fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_upload_head() {
        let req = b"PUT /upload?name=Crash%20Bandicoot%20(Track%201).bin&x=a+b HTTP/1.1\r\nHost: 192.168.1.5:8080\r\nContent-Length: 12345\r\n\r\nBODY";
        let head = parse_head(req).unwrap().unwrap();
        assert_eq!(head.method, "PUT");
        assert_eq!(head.path, "/upload");
        assert_eq!(head.query_value("name"), Some("Crash Bandicoot (Track 1).bin"));
        assert_eq!(head.query_value("x"), Some("a b"));
        assert_eq!(head.content_length, 12345);
        assert_eq!(head.header("HOST"), Some("192.168.1.5:8080"));
        assert_eq!(&req[head.head_len..], b"BODY");
    }

    #[test]
    fn incomplete_and_bad_heads() {
        assert_eq!(parse_head(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap(), None);
        assert!(parse_head(b"hello world\r\n\r\n").is_err());
        assert!(parse_head(b"PUT /u HTTP/1.1\r\nContent-Length: lots\r\n\r\n").is_err());
        assert!(parse_head(&vec![b'a'; MAX_HEAD + 1]).is_err());
    }

    #[test]
    fn upload_names_cannot_escape() {
        assert_eq!(safe_file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_file_name("C:\\games\\Zelda.sfc").as_deref(), Some("Zelda.sfc"));
        assert_eq!(safe_file_name(".."), None);
        assert_eq!(safe_file_name(".hidden"), None);
        assert_eq!(safe_file_name("a\nb"), None);
    }

    #[test]
    fn response_and_json() {
        let head = String::from_utf8(response_head(200, "text/html", 5, &[("X-A".into(), "1".into())])).unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Length: 5\r\n"));
        assert!(head.ends_with("X-A: 1\r\n\r\n"));
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
        let redirect = String::from_utf8(response_head(302, "text/plain", 0, &[("Location".into(), "/k7m2qx/".into())])).unwrap();
        assert!(redirect.starts_with("HTTP/1.1 302 Found\r\n"));
        assert!(redirect.contains("Location: /k7m2qx/\r\n"));
    }

    const CODE: &str = "k7m2qx";

    /// The request target as a browser sends it, through the real head parser, then the gate.
    fn gate(target: &str) -> Option<String> {
        let req = format!("GET {target} HTTP/1.1\r\nHost: 192.168.1.20:8080\r\n\r\n");
        let head = parse_head(req.as_bytes()).unwrap().unwrap();
        strip_access_code(&head.path, CODE)
    }

    #[test]
    fn the_right_code_routes_the_remainder() {
        assert_eq!(strip_access_code("/k7m2qx/", CODE).as_deref(), Some("/"));
        assert_eq!(strip_access_code("/k7m2qx/index.html", CODE).as_deref(), Some("/index.html"));
        assert_eq!(strip_access_code("/k7m2qx/api/library", CODE).as_deref(), Some("/api/library"));
        assert_eq!(strip_access_code("/k7m2qx/saves/Zelda.srm", CODE).as_deref(), Some("/saves/Zelda.srm"));
        assert_eq!(strip_access_code("/k7m2qx/upload", CODE).as_deref(), Some("/upload"));
        // A capital typed by mistake still matches; the alphabet is lowercase only.
        assert_eq!(strip_access_code("/K7M2QX/api/library", CODE).as_deref(), Some("/api/library"));
    }

    #[test]
    fn a_wrong_or_missing_code_is_refused() {
        for path in [
            "/",
            "/index.html",
            "/api/library",
            "/saves/Zelda.srm",
            "/upload",
            "/k7m2qy/",          // last character wrong
            "/a7m2qx/",          // first character wrong
            "/k7m2q/",           // too short
            "/k7m2qxx/",         // too long
            "/k7m2qxapi/library", // code run into the route
            "/xk7m2qx/",
            "//k7m2qx/",
            "/api/k7m2qx/",
            "k7m2qx/",           // no leading slash
            "",
            "*",
            "http://192.168.1.20:8080/k7m2qx/",
        ] {
            assert_eq!(strip_access_code(path, CODE), None, "{path}");
        }
    }

    #[test]
    fn code_without_a_trailing_slash_asks_for_a_redirect() {
        assert_eq!(strip_access_code("/k7m2qx", CODE).as_deref(), Some(""));
        assert_eq!(gate("/k7m2qx").as_deref(), Some(""));
        assert_eq!(gate("/k7m2qx?from=phone").as_deref(), Some(""));
    }

    #[test]
    fn query_strings_stay_out_of_the_check() {
        let req = b"PUT /k7m2qx/upload?name=Crash%20Bandicoot%20(Track%201).bin HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc";
        let head = parse_head(req).unwrap().unwrap();
        assert_eq!(strip_access_code(&head.path, CODE).as_deref(), Some("/upload"));
        assert_eq!(head.query_value("name"), Some("Crash Bandicoot (Track 1).bin"));
        assert_eq!(gate("/k7m2qx/api/library?t=123").as_deref(), Some("/api/library"));
        // The code in the query instead of the path does not count.
        assert_eq!(gate("/upload?code=k7m2qx&name=a.bin"), None);
        assert_eq!(gate("/?k7m2qx"), None);
        // A decoded `?` is part of the path, not the start of a query.
        assert_eq!(gate("/k7m2qx%3F/"), None);
    }

    #[test]
    fn percent_encoding_is_decoded_before_the_check() {
        // The remainder comes back decoded, as the routes expect.
        assert_eq!(gate("/k7m2qx/saves/My%20Game%20(USA).srm").as_deref(), Some("/saves/My Game (USA).srm"));
        // An encoded code is still the code (whoever sent it knew it).
        assert_eq!(gate("/%6B%37m2qx/").as_deref(), Some("/"));
        // An encoded slash splits the segment, so it is not the code.
        assert_eq!(gate("/k7m%2F2qx/"), None);
        assert_eq!(gate("/%2Fk7m2qx/"), None);
        // Traversal after the code stays after the code, and the routes refuse it.
        assert_eq!(gate("/k7m2qx/..%2F..%2Fapi/library").as_deref(), Some("/../../api/library"));
        assert_eq!(gate("/k7m2qx/%2E%2E/").as_deref(), Some("/../"));
        assert_eq!(gate("/%2E%2E/k7m2qx/"), None);
    }

    #[test]
    fn a_malformed_code_opens_nothing() {
        // Fail closed: if the app ever passed an empty or broken code, nothing would match.
        for code in ["", "k7m2q", "k7m2qxx", "k7m2q0", "K7M2QX", "k7m2q/", "k7m2q?"] {
            assert!(!is_access_code(code), "{code}");
            assert_eq!(strip_access_code("/", code), None, "{code}");
            assert_eq!(strip_access_code(&format!("/{code}/"), code), None, "{code}");
            assert_eq!(strip_access_code(&format!("/{code}"), code), None, "{code}");
        }
        assert_eq!(strip_access_code("//", ""), None);
    }

    #[test]
    fn access_codes_use_the_unambiguous_alphabet() {
        assert_eq!(ACCESS_CODE_ALPHABET.len(), 31);
        for confusable in ['0', 'o', '1', 'l', 'i'] {
            assert!(!ACCESS_CODE_ALPHABET.contains(confusable), "{confusable}");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(ACCESS_CODE_ALPHABET.chars().all(|c| seen.insert(c)), "a character is listed twice");
        assert!(is_access_code(CODE));
        assert!(is_access_code("23456z"));
        assert!(!is_access_code("k7m2qé"));
    }

    #[test]
    fn timing_safe_compare_is_correct() {
        assert!(constant_time_eq(b"k7m2qx", b"k7m2qx"));
        assert!(constant_time_eq(b"", b""));
        // A difference anywhere, first or last, is a difference.
        assert!(!constant_time_eq(b"k7m2qx", b"a7m2qx"));
        assert!(!constant_time_eq(b"k7m2qx", b"k7m2qy"));
        assert!(!constant_time_eq(b"k7m2qx", b"k7m2q"));
        assert!(!constant_time_eq(b"k7m2q", b"k7m2qx"));
        // Every byte is looked at: two inputs that differ only in a single bit of the last byte.
        assert!(!constant_time_eq(&[0u8; 64], &{
            let mut b = [0u8; 64];
            b[63] = 1;
            b
        }));
    }

    #[test]
    fn the_address_ends_in_a_slash() {
        let address = transfer_address("192.168.1.20", 8080, CODE);
        assert_eq!(address, "http://192.168.1.20:8080/k7m2qx/");
        assert!(address.ends_with('/'));
        // What a browser asks for when the address is typed is exactly what the gate opens.
        let path = &address["http://192.168.1.20:8080".len()..];
        assert_eq!(gate(path).as_deref(), Some("/"));
    }

    #[test]
    fn the_page_uses_relative_urls() {
        // Relative to `/<code>/`, so every request the page makes carries the code.
        assert!(PAGE.contains("'upload?name='"));
        assert!(PAGE.contains("'api/library'"));
        assert!(PAGE.contains("'saves/'"));
        for absolute in ["'/upload", "'/api/", "'/saves/", "\"/upload", "\"/api/", "\"/saves/", "fetch('/", "open('PUT', '/"] {
            assert!(!PAGE.contains(absolute), "the page still has {absolute}");
        }
    }
}
