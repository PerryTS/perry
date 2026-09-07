use crate::simd::find_string_terminator;
pub(crate) enum ParsedStr<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> ParsedStr<'a> {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            ParsedStr::Borrowed(s) => s,
            ParsedStr::Owned(v) => v,
        }
    }
}

#[inline]
fn decode_hex_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    let mut value = 0u16;
    for &byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        value = (value << 4) | digit as u16;
    }
    Some(value)
}

#[inline]
fn push_code_unit_wtf8(output: &mut Vec<u8>, unit: u16) {
    if (0xD800..=0xDFFF).contains(&unit) {
        output.push(0xE0 | (unit >> 12) as u8);
        output.push(0x80 | ((unit >> 6) & 0x3F) as u8);
        output.push(0x80 | (unit & 0x3F) as u8);
    } else {
        let ch =
            char::from_u32(unit as u32).expect("non-surrogate UTF-16 unit is a Unicode scalar");
        let mut buf = [0u8; 4];
        output.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }
}


pub struct DirectParser<'a> { pub input: &'a [u8], pub pos: usize, pub valid: bool }
impl<'a> DirectParser<'a> {
 pub fn new(input: &'a [u8]) -> Self {Self {input,pos:0,valid:true}}
 fn peek(&self) -> Option<u8> {self.input.get(self.pos).copied()}
 fn advance(&mut self) {self.pos+=1;}
    pub(crate) fn parse_string_bytes(&mut self) -> Option<ParsedStr<'a>> {
        if self.peek() != Some(b'"') {
            self.valid = false;
            return None;
        }
        self.advance();
        let start = self.pos;

        // SIMD-accelerated scan for `"` or `\`. On match, fall through
        // to the scalar loop which positions `self.pos` exactly.
        if let Some(hit) = find_string_terminator(&self.input[self.pos..]) {
            // `hit` is the offset within the remaining slice of the
            // first `"` or `\`. If it's `"`, we're done; if `\`, slow
            // path picks up from the current position.
            self.pos += hit;
            let ch = self.input[self.pos];
            if ch == b'"' {
                let slice = &self.input[start..self.pos];
                self.pos += 1;
                return Some(ParsedStr::Borrowed(slice));
            }
            if ch < 0x20 {
                self.valid = false;
                return None;
            }
            // ch == b'\\' — slow path from here.
            return self.parse_string_bytes_slow(start);
        }
        self.valid = false;
        None
    }

    pub(crate) fn parse_string_bytes_slow(&mut self, start: usize) -> Option<ParsedStr<'a>> {
        let mut result = Vec::from(&self.input[start..self.pos]);
        while self.input.len() - self.pos >= 64 {
            match self.decode_chunk(&mut result)? {
                true => return Some(ParsedStr::Owned(result)),
                false => {}
            }
        }
        loop {
            if self.pos >= self.input.len() {
                self.valid = false;
                return None;
            }
            let ch = self.input[self.pos];
            self.pos += 1;
            match ch {
                b'"' => return Some(ParsedStr::Owned(result)),
                b'\\' => {
                    if self.pos >= self.input.len() {
                        self.valid = false;
                        return None;
                    }
                    let esc = self.input[self.pos];
                    self.pos += 1;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            if self.pos + 4 > self.input.len() {
                                self.valid = false;
                                return None;
                            }
                            let Some(code) = decode_hex_u16(&self.input[self.pos..self.pos + 4])
                            else {
                                self.valid = false;
                                return None;
                            };
                            self.pos += 4;
                            let paired_low = if (0xD800..=0xDBFF).contains(&code)
                                && self.pos + 6 <= self.input.len()
                                && self.input[self.pos] == b'\\'
                                && self.input[self.pos + 1] == b'u'
                            {
                                decode_hex_u16(&self.input[self.pos + 2..self.pos + 6])
                                    .filter(|low| (0xDC00..=0xDFFF).contains(low))
                            } else {
                                None
                            };
                            if let Some(low) = paired_low {
                                self.pos += 6;
                                let codepoint = 0x10000
                                    + ((code as u32 - 0xD800) << 10)
                                    + (low as u32 - 0xDC00);
                                let c = char::from_u32(codepoint)
                                    .expect("surrogate pair is a Unicode scalar");
                                let mut buf = [0u8; 4];
                                let s = c.encode_utf8(&mut buf);
                                result.extend_from_slice(s.as_bytes());
                            } else {
                                push_code_unit_wtf8(&mut result, code);
                            }
                        }
                        _ => {
                            self.valid = false;
                            return None;
                        }
                    }
                }
                c if c < 0x20 => {
                    self.valid = false;
                    return None;
                }
                _ => result.push(ch),
            }
        }
    }

    /// Decode a bounded input window into reserved Vec storage. A JSON escape
    /// consumes at most 12 bytes and emits at most four. Starting each operation
    /// before byte 52 of a complete 64-byte window bounds every input access and
    /// every output write. The Vec never reallocates while its pointer is used.
    fn decode_chunk(&mut self, result: &mut Vec<u8>) -> Option<bool> {
        debug_assert!(self.input.len() - self.pos >= 64);
        let used = result.len();
        if result.capacity() - used < 64 {
            let capacity = (used + 64).next_power_of_two();
            result.reserve_exact(capacity - used);
        }
        let output = unsafe { result.as_mut_ptr().add(used) };
        let mut written = 0usize;
        let mut pos = self.pos;
        let limit = pos + 52;
        macro_rules! invalid {
            () => {{ self.pos = pos; self.valid = false; return None; }};
        }
        macro_rules! put {
            ($byte:expr) => {{ unsafe { output.add(written).write($byte); } written += 1; }};
        }
        while pos < limit {
            let ch = unsafe { *self.input.get_unchecked(pos) };
            pos += 1;
            match ch {
                b'"' => {
                    self.pos = pos;
                    unsafe { result.set_len(used + written); }
                    return Some(true);
                }
                b'\\' => {
                    let esc = unsafe { *self.input.get_unchecked(pos) };
                    pos += 1;
                    match esc {
                        b'"' => put!(b'"'),
                        b'\\' => put!(b'\\'),
                        b'/' => put!(b'/'),
                        b'n' => put!(b'\n'),
                        b'r' => put!(b'\r'),
                        b't' => put!(b'\t'),
                        b'b' => put!(0x08),
                        b'f' => put!(0x0c),
                        b'u' => {
                            let Some(code) = decode_hex_u16(unsafe { self.input.get_unchecked(pos..pos + 4) }) else { invalid!(); };
                            pos += 4;
                            let low = if (0xd800..=0xdbff).contains(&code)
                                && self.input[pos] == b'\\'
                                && self.input[pos + 1] == b'u'
                            {
                                decode_hex_u16(unsafe { self.input.get_unchecked(pos + 2..pos + 6) })
                                    .filter(|low| (0xdc00..=0xdfff).contains(low))
                            } else { None };
                            let mut bytes = [0u8; 4];
                            let length = if let Some(low) = low {
                                pos += 6;
                                let scalar = 0x10000 + ((code as u32 - 0xd800) << 10) + (low as u32 - 0xdc00);
                                char::from_u32(scalar).unwrap().encode_utf8(&mut bytes).len()
                            } else if (0xd800..=0xdfff).contains(&code) {
                                bytes[0] = 0xe0 | (code >> 12) as u8;
                                bytes[1] = 0x80 | ((code >> 6) & 0x3f) as u8;
                                bytes[2] = 0x80 | (code & 0x3f) as u8;
                                3
                            } else {
                                char::from_u32(code as u32).unwrap().encode_utf8(&mut bytes).len()
                            };
                            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.add(written), length); }
                            written += length;
                        }
                        _ => invalid!(),
                    }
                }
                c if c < 0x20 => invalid!(),
                _ => put!(ch),
            }
        }
        self.pos = pos;
        unsafe { result.set_len(used + written); }
        Some(false)
    }
}
