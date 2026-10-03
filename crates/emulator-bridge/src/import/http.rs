//! The Wi-Fi transfer server's protocol half: request heads in, response heads out, and the page.
//!
//! The socket is the app's (NWListener on iOS). Bodies never pass through here: an upload is a
//! plain `PUT /upload?name=<file>` whose body the app streams straight to a file, so a 700 MB disc
//! track costs a buffer rather than 700 MB of memory. Only the head (request line and headers,
//! capped at [`MAX_HEAD`]) is parsed in Rust.

use super::webdav::percent_decode;

/// The largest request head accepted. Browsers send well under 8 KB.
pub const MAX_HEAD: usize = 16 * 1024;

/// The upload page, served at `/`. Plain HTML and script with no outside resources, so it works on
/// a network with no internet.
pub const PAGE: &str = include_str!("wifi_page.html");

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
        assert!(PAGE.contains("/upload"));
    }
}
