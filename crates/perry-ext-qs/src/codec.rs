#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Charset {
    Utf8,
    Latin1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Rfc1738,
    Rfc3986,
}

pub(crate) fn encode(input: &str, charset: Charset, format: Format) -> String {
    let mut out = String::with_capacity(input.len());
    match charset {
        Charset::Utf8 => {
            for &byte in input.as_bytes() {
                if is_safe(byte, format) {
                    out.push(byte as char);
                } else {
                    push_escape(&mut out, byte);
                }
            }
        }
        Charset::Latin1 => {
            for unit in input.encode_utf16() {
                if unit <= 0xFF {
                    let byte = unit as u8;
                    if is_safe(byte, format) {
                        out.push(byte as char);
                    } else {
                        push_escape(&mut out, byte);
                    }
                } else {
                    out.push_str("%26%23");
                    out.push_str(&unit.to_string());
                    out.push_str("%3B");
                }
            }
        }
    }
    if format == Format::Rfc1738 {
        out = out.replace("%20", "+");
    }
    out
}

pub(crate) fn format_encoded(input: String, format: Format) -> String {
    if format == Format::Rfc1738 {
        input.replace("%20", "+")
    } else {
        input
    }
}

pub(crate) fn decode(input: &str, charset: Charset) -> String {
    let plus_replaced = input.replace('+', " ");
    let mut bytes = Vec::with_capacity(plus_replaced.len());
    let raw = plus_replaced.as_bytes();
    let mut index = 0;
    let mut invalid_escape = false;
    while index < raw.len() {
        if raw[index] == b'%' {
            if index + 2 < raw.len() {
                if let (Some(high), Some(low)) = (hex(raw[index + 1]), hex(raw[index + 2])) {
                    bytes.push((high << 4) | low);
                    index += 3;
                    continue;
                }
            }
            invalid_escape = true;
        }
        bytes.push(raw[index]);
        index += 1;
    }

    match charset {
        Charset::Utf8 if invalid_escape => plus_replaced,
        Charset::Utf8 => String::from_utf8(bytes).unwrap_or(plus_replaced),
        Charset::Latin1 => bytes.into_iter().map(char::from).collect(),
    }
}

fn is_safe(byte: u8, format: Format) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'-' | b'.' | b'_' | b'~')
        || (format == Format::Rfc1738 && matches!(byte, b'(' | b')'))
}

fn push_escape(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0xF) as usize] as char);
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3986_encoding_matches_qs_defaults() {
        assert_eq!(
            encode("a b[c]/✓", Charset::Utf8, Format::Rfc3986),
            "a%20b%5Bc%5D%2F%E2%9C%93"
        );
    }

    #[test]
    fn rfc1738_uses_plus_and_preserves_parentheses() {
        assert_eq!(encode("a b(c)", Charset::Utf8, Format::Rfc1738), "a+b(c)");
    }

    #[test]
    fn decoder_is_lenient_like_decode_uri_component_wrapper() {
        assert_eq!(decode("a+b%5Bc%5D", Charset::Utf8), "a b[c]");
        assert_eq!(decode("bad%ZZ", Charset::Utf8), "bad%ZZ");
    }
}
