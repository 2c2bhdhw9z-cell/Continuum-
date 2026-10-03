//! WebDAV folder listing: the PROPFIND body to send and the multistatus answer, parsed.
//!
//! The parser is deliberately small and forgiving rather than a full XML parser: servers disagree
//! about namespace prefixes (`D:`, `d:`, `lp1:`, none), and all that is needed out of a listing is
//! each response's href, whether it is a collection, its size and its date.

/// The body of a `PROPFIND` with `Depth: 1`.
pub const PROPFIND_BODY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<d:propfind xmlns:d=\"DAV:\"><d:prop><d:displayname/><d:resourcetype/>\
<d:getcontentlength/><d:getlastmodified/></d:prop></d:propfind>";

/// One entry in a WebDAV folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavEntry {
    /// The decoded path on the server, as the href gave it (no scheme or host).
    pub path: String,
    /// The last path component, decoded.
    pub name: String,
    pub is_folder: bool,
    /// Bytes, or 0 when the server did not say.
    pub size: u64,
    /// The server's date text, as sent.
    pub modified: String,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(end) = tail.find(';') {
            let entity = &tail[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16).ok().and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = ch {
                out.push(c);
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Percent-decodes a URL path. Invalid escapes are kept as written.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Strips `scheme://host` from an href, leaving the path.
fn href_path(href: &str) -> String {
    let href = href.trim();
    let path = match href.find("://") {
        Some(i) => match href[i + 3..].find('/') {
            Some(j) => &href[i + 3 + j..],
            None => "/",
        },
        None => href,
    };
    percent_decode(path)
}

/// Parses a `207 Multi-Status` body. The folder that was asked about (`request_path`, decoded or
/// not) is left out, folders come first, then files, each sorted by name.
pub fn parse_multistatus(xml: &str, request_path: &str) -> Vec<DavEntry> {
    let mut entries = Vec::new();
    let mut current: Option<DavEntry> = None;
    let mut element = String::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];
        if let Some(entry) = current.as_mut() {
            let value = decode_entities(text.trim());
            if !value.is_empty() {
                match element.as_str() {
                    "href" => entry.path.push_str(&value),
                    "getcontentlength" => entry.size = value.parse().unwrap_or(0),
                    "getlastmodified" => entry.modified = value,
                    _ => {}
                }
            }
        }
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        let name = local(name).to_ascii_lowercase();
        if closing {
            if name == "response" {
                if let Some(mut entry) = current.take() {
                    entry.path = href_path(&entry.path);
                    let trimmed = entry.path.trim_end_matches('/');
                    entry.name = trimmed.rsplit('/').next().unwrap_or("").to_string();
                    entries.push(entry);
                }
            }
            element.clear();
        } else {
            if name == "response" {
                current = Some(DavEntry { path: String::new(), name: String::new(), is_folder: false, size: 0, modified: String::new() });
            } else if name == "collection" {
                if let Some(entry) = current.as_mut() {
                    entry.is_folder = true;
                }
            }
            element = if self_closing { String::new() } else { name };
        }
    }
    let asked = href_path(request_path);
    let asked = asked.trim_end_matches('/');
    entries.retain(|e| e.path.trim_end_matches('/') != asked && !e.name.is_empty());
    entries.sort_by(|a, b| {
        b.is_folder
            .cmp(&a.is_folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEXTCLOUD: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns">
 <d:response><d:href>/remote.php/dav/files/me/Games/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Games/PS1%20Discs/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>
  <d:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</d:getlastmodified></d:prop></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Games/Tom%20%26%20Jerry.gba</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>4194304</d:getcontentlength></d:prop></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/me/Games/A&amp;B.nes</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>40976</d:getcontentlength></d:prop></d:propstat></d:response>
</d:multistatus>"#;

    #[test]
    fn parses_nextcloud_listing() {
        let entries = parse_multistatus(NEXTCLOUD, "/remote.php/dav/files/me/Games/");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "PS1 Discs");
        assert!(entries[0].is_folder);
        assert_eq!(entries[0].modified, "Mon, 01 Jan 2024 00:00:00 GMT");
        assert_eq!(entries[1].name, "A&B.nes");
        assert_eq!(entries[2].name, "Tom & Jerry.gba");
        assert_eq!(entries[2].size, 4194304);
        assert_eq!(entries[2].path, "/remote.php/dav/files/me/Games/Tom & Jerry.gba");
    }

    #[test]
    fn handles_absolute_hrefs_and_no_prefix() {
        let xml = r#"<multistatus xmlns="DAV:"><response><href>https://nas.local:5006/share/</href>
<propstat><prop><resourcetype><collection/></resourcetype></prop></propstat></response>
<response><href>https://nas.local:5006/share/x.sfc</href><propstat><prop><getcontentlength>10</getcontentlength><resourcetype/></prop></propstat></response></multistatus>"#;
        let entries = parse_multistatus(xml, "/share");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/share/x.sfc");
        assert!(!entries[0].is_folder);
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("a%20b%2Fc%zz%"), "a b/c%zz%");
        assert_eq!(decode_entities("&lt;&#65;&#x42;&bogus;"), "<AB&bogus;");
    }
}
