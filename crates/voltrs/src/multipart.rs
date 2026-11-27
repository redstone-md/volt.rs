//! `multipart/form-data` parser over a fully collected (size-limited) request body.
//! Parts are zero-copy slices of the body.

use bytes::Bytes;
use memchr::memmem;

use crate::form::FormError;

/// One part of a multipart body.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Bytes,
}

/// Splits a multipart body into its parts using the boundary from `content_type`.
pub(crate) fn parse(content_type: &str, body: &Bytes) -> Result<Vec<Part>, FormError> {
    let boundary = boundary(content_type).ok_or_else(|| FormError::new("multipart body without a boundary"))?;
    let delimiter = format!("\r\n--{boundary}");
    let finder = memmem::Finder::new(delimiter.as_bytes());
    let opening = &delimiter.as_bytes()[2..];
    let mut cursor = if body.starts_with(opening) {
        opening.len()
    } else {
        finder.find(body).ok_or_else(|| FormError::new("multipart boundary not found"))? + delimiter.len()
    };
    let mut parts = Vec::new();
    loop {
        let rest = &body[cursor..];
        if rest.starts_with(b"--") {
            return Ok(parts);
        }
        let padding = rest.iter().take_while(|b| matches!(b, b' ' | b'\t')).count();
        if !rest[padding..].starts_with(b"\r\n") {
            return Err(FormError::new("malformed multipart delimiter"));
        }
        let start = cursor + padding + 2;
        let end = finder
            .find(&body[start..])
            .map(|offset| start + offset)
            .ok_or_else(|| FormError::new("unterminated multipart body"))?;
        if let Some(part) = part(body, start, end)? {
            parts.push(part);
        }
        cursor = end + delimiter.len();
    }
}

fn part(body: &Bytes, start: usize, end: usize) -> Result<Option<Part>, FormError> {
    let raw = &body[start..end];
    let (head, content) = if raw.starts_with(b"\r\n") {
        (&raw[..0], start + 2)
    } else {
        let split = memmem::find(raw, b"\r\n\r\n").ok_or_else(|| FormError::new("multipart part without headers"))?;
        (&raw[..split], start + split + 4)
    };
    let mut disposition = None;
    let mut content_type = None;
    for line in String::from_utf8_lossy(head).split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.trim().eq_ignore_ascii_case("content-disposition") {
            disposition = Some(Disposition::parse(value));
        } else if name.trim().eq_ignore_ascii_case("content-type") {
            content_type = Some(value.to_owned());
        }
    }
    let disposition = disposition.ok_or_else(|| FormError::new("multipart part without Content-Disposition"))?;
    let Some(name) = disposition.name else {
        return Ok(None);
    };
    let data = body.slice(content..end);
    // Browsers send an empty, unnamed file part for a file input left blank.
    if disposition.filename.as_deref() == Some("") && data.is_empty() {
        return Ok(None);
    }
    Ok(Some(Part { name, filename: disposition.filename, content_type, data }))
}

#[derive(Default)]
struct Disposition {
    name: Option<String>,
    filename: Option<String>,
}

impl Disposition {
    fn parse(value: &str) -> Self {
        let mut out = Self::default();
        let mut extended = None;
        for (key, value) in params(value) {
            match key.to_ascii_lowercase().as_str() {
                "name" => out.name = Some(unescape(&value)),
                "filename" => out.filename = Some(basename(&unescape(&value))),
                "filename*" => extended = decode_extended(&value),
                _ => {}
            }
        }
        if let Some(filename) = extended {
            out.filename = Some(basename(&filename));
        }
        out
    }
}

/// Parses `; key=value` parameters, honoring quoted strings with backslash escapes.
fn params(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = value.chars().peekable();
    while chars.peek().is_some() {
        let mut key = String::new();
        let mut has_value = false;
        for c in chars.by_ref() {
            match c {
                '=' => {
                    has_value = true;
                    break;
                }
                ';' => break,
                c => key.push(c),
            }
        }
        let key = key.trim().to_owned();
        if !has_value {
            continue;
        }
        let mut val = String::new();
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => val.extend(chars.next()),
                    '"' => break,
                    c => val.push(c),
                }
            }
            for c in chars.by_ref() {
                if c == ';' {
                    break;
                }
            }
        } else {
            val = chars.by_ref().take_while(|c| *c != ';').collect::<String>().trim().to_owned();
        }
        if !key.is_empty() {
            out.push((key, val));
        }
    }
    out
}

/// Browsers percent-encode `"`, CR and LF in field and file names.
fn unescape(value: &str) -> String {
    value.replace("%22", "\"").replace("%0D", "\r").replace("%0A", "\n")
}

/// RFC 5987 `filename*=UTF-8''name`.
fn decode_extended(value: &str) -> Option<String> {
    let (charset, rest) = value.split_once('\'')?;
    let (_, encoded) = rest.split_once('\'')?;
    charset
        .eq_ignore_ascii_case("utf-8")
        .then(|| percent_encoding::percent_decode_str(encoded).decode_utf8_lossy().into_owned())
}

/// Keeps the last path segment without control characters; `.` and `..` become empty so the
/// name can never point at a directory when joined onto one.
fn basename(path: &str) -> String {
    let name: String = path.rsplit(['/', '\\']).next().unwrap_or(path).chars().filter(|c| !c.is_control()).collect();
    if matches!(name.trim(), "." | "..") { String::new() } else { name }
}

fn boundary(content_type: &str) -> Option<String> {
    let (_, rest) = content_type.split_once(';')?;
    params(rest)
        .into_iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("boundary"))
        .map(|(_, value)| value)
        .filter(|b| !b.is_empty() && b.len() <= 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPE: &str = "multipart/form-data; boundary=----volt";

    fn body(parts: &[&str]) -> Bytes {
        let mut out = String::new();
        for part in parts {
            out.push_str("------volt\r\n");
            out.push_str(part);
            out.push_str("\r\n");
        }
        out.push_str("------volt--\r\n");
        Bytes::from(out)
    }

    #[test]
    fn parses_text_and_files() {
        let body = body(&[
            "Content-Disposition: form-data; name=\"title\"\r\n\r\nHello\r\nworld",
            "Content-Disposition: form-data; name=\"file\"; filename=\"C:\\\\docs\\\\a.txt\"\r\nContent-Type: text/plain\r\n\r\nabc",
            "Content-Disposition: form-data; name=\"empty\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n",
        ]);
        let parts = parse(TYPE, &body).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "title");
        assert_eq!(&parts[0].data[..], b"Hello\r\nworld");
        assert_eq!(parts[1].filename.as_deref(), Some("a.txt"));
        assert_eq!(parts[1].content_type.as_deref(), Some("text/plain"));
        assert_eq!(&parts[1].data[..], b"abc");
    }

    #[test]
    fn handles_quoted_boundaries_and_preamble() {
        let raw = "preamble\r\n--x y\r\nContent-Disposition: form-data; name=a\r\n\r\n1\r\n--x y--";
        let parts = parse("multipart/form-data; boundary=\"x y\"", &Bytes::from(raw)).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(&parts[0].data[..], b"1");
    }

    #[test]
    fn decodes_extended_filenames() {
        let body = body(&["Content-Disposition: form-data; name=f; filename*=UTF-8''%C3%A9t%C3%A9.png\r\n\r\nx"]);
        assert_eq!(parse(TYPE, &body).unwrap()[0].filename.as_deref(), Some("été.png"));
    }

    #[test]
    fn sanitizes_filenames() {
        let body = body(&[
            "Content-Disposition: form-data; name=a; filename=\"x/..\"\r\n\r\n1",
            "Content-Disposition: form-data; name=b; filename=\"a\0b\tc.txt\"\r\n\r\n2",
            "Content-Disposition: form-data; name=c; filename*=UTF-8''..%2F..%00\r\n\r\n3",
        ]);
        let names: Vec<_> = parse(TYPE, &body).unwrap().into_iter().map(|p| p.filename.unwrap()).collect();
        assert_eq!(names, ["", "abc.txt", ""]);
    }

    #[test]
    fn rejects_truncated_bodies() {
        let raw = Bytes::from("------volt\r\nContent-Disposition: form-data; name=a\r\n\r\nno end");
        assert!(parse(TYPE, &raw).is_err());
        assert!(parse("multipart/form-data", &raw).is_err());
    }
}
