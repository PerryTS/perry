//! V8's `JSON.parse` SyntaxError messages (json-parser.cc `ReportUnexpectedToken`
//! and the `kJsonParse*` message templates). The fast parsers only record that
//! a parse failed; on that cold path this scanner re-reads the source with
//! V8's grammar walk and names the first error the way V8 does: a contextual
//! message where V8 expects a specific token (`Expected ',' or ']' after array
//! element`), a token message elsewhere (`Unexpected token 'x', "..." is not
//! valid JSON`, with V8's 10-unit source context once the source is longer
//! than 20 units), and `Unexpected end of JSON input` for an early end.
//! Positions, the line/column pair and the context excerpt are in UTF-16 code
//! units; the source and the returned message are WTF-8.

/// V8's `kMaxContextCharacters`.
const CONTEXT: usize = 10;
/// Sources up to this length are quoted whole (V8's
/// `kMinOriginalSourceLengthForContext` is 21: shorter sources get no context).
const SHORT_SOURCE: usize = 20;

enum Failure {
    /// `Unexpected end of JSON input`.
    End,
    /// A token V8 did not expect where it wanted a value (or a literal's next
    /// letter): reported by the token at `at`.
    Token(usize),
    /// A leading-zero number continued by a digit at `at`.
    Number(usize),
    /// A contextual template (the text before " in JSON at position").
    At(usize, &'static str),
}

struct Scanner<'a> {
    units: &'a [u16],
    pos: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u16> {
        self.units.get(self.pos).copied()
    }
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0A | 0x0D)) {
            self.pos += 1;
        }
    }
    fn is_digit(unit: Option<u16>) -> bool {
        matches!(unit, Some(c) if (u16::from(b'0')..=u16::from(b'9')).contains(&c))
    }

    /// The whole text: one value, then only whitespace.
    fn text(&mut self) -> Result<(), Failure> {
        self.skip_whitespace();
        self.value()?;
        self.skip_whitespace();
        match self.peek() {
            None => Ok(()),
            Some(_) => Err(Failure::At(
                self.pos,
                "Unexpected non-whitespace character after JSON",
            )),
        }
    }

    /// A value at the cursor (whitespace already skipped).
    fn value(&mut self) -> Result<(), Failure> {
        match self.peek() {
            None => Err(Failure::End),
            Some(0x22) => self.string(),
            Some(0x7B) => self.object(),
            Some(0x5B) => self.array(),
            Some(0x74) => self.literal(b"true"),
            Some(0x66) => self.literal(b"false"),
            Some(0x6E) => self.literal(b"null"),
            Some(c) if c == u16::from(b'-') || Self::is_digit(Some(c)) => self.number(),
            Some(_) => Err(Failure::Token(self.pos)),
        }
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), Failure> {
        for &letter in word {
            match self.peek() {
                None => return Err(Failure::End),
                Some(c) if c == u16::from(letter) => self.pos += 1,
                Some(_) => return Err(Failure::Token(self.pos)),
            }
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), Failure> {
        if self.peek() == Some(u16::from(b'-')) {
            self.pos += 1;
            if !Self::is_digit(self.peek()) {
                return Err(Failure::At(self.pos, "No number after minus sign"));
            }
        }
        if self.peek() == Some(u16::from(b'0')) {
            self.pos += 1;
            if Self::is_digit(self.peek()) {
                return Err(Failure::Number(self.pos));
            }
        } else {
            while Self::is_digit(self.peek()) {
                self.pos += 1;
            }
        }
        if self.peek() == Some(u16::from(b'.')) {
            self.pos += 1;
            if !Self::is_digit(self.peek()) {
                return Err(Failure::At(self.pos, "Unterminated fractional number"));
            }
            while Self::is_digit(self.peek()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(c) if c == u16::from(b'e') || c == u16::from(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(c) if c == u16::from(b'+') || c == u16::from(b'-')) {
                self.pos += 1;
            }
            if !Self::is_digit(self.peek()) {
                return Err(Failure::At(self.pos, "Exponent part is missing a number"));
            }
            while Self::is_digit(self.peek()) {
                self.pos += 1;
            }
        }
        Ok(())
    }

    fn string(&mut self) -> Result<(), Failure> {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return Err(Failure::At(self.pos, "Unterminated string")),
                Some(0x22) => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(c) if c < 0x20 => {
                    return Err(Failure::At(
                        self.pos,
                        "Bad control character in string literal",
                    ))
                }
                Some(0x5C) => {
                    self.pos += 1;
                    match self.peek() {
                        None => return Err(Failure::End),
                        Some(c) if b"\"\\/bfnrt".iter().any(|&e| u16::from(e) == c) => {
                            self.pos += 1;
                        }
                        Some(0x75) => {
                            self.pos += 1;
                            for _ in 0..4 {
                                match self.peek() {
                                    Some(c)
                                        if char::from_u32(u32::from(c))
                                            .is_some_and(|c| c.is_ascii_hexdigit()) =>
                                    {
                                        self.pos += 1
                                    }
                                    _ => return Err(Failure::At(self.pos, "Bad Unicode escape")),
                                }
                            }
                        }
                        Some(_) => return Err(Failure::At(self.pos, "Bad escaped character")),
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn object(&mut self) -> Result<(), Failure> {
        self.pos += 1;
        self.skip_whitespace();
        if self.peek() == Some(0x7D) {
            self.pos += 1;
            return Ok(());
        }
        let mut first = true;
        loop {
            if self.peek() != Some(0x22) {
                return Err(Failure::At(
                    self.pos,
                    if first {
                        "Expected property name or '}'"
                    } else {
                        "Expected double-quoted property name"
                    },
                ));
            }
            first = false;
            self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(u16::from(b':')) {
                return Err(Failure::At(self.pos, "Expected ':' after property name"));
            }
            self.pos += 1;
            self.skip_whitespace();
            self.value()?;
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                Some(0x7D) => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => {
                    return Err(Failure::At(
                        self.pos,
                        "Expected ',' or '}' after property value",
                    ))
                }
            }
        }
    }

    fn array(&mut self) -> Result<(), Failure> {
        self.pos += 1;
        self.skip_whitespace();
        if self.peek() == Some(0x5D) {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                Some(0x5D) => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => {
                    return Err(Failure::At(
                        self.pos,
                        "Expected ',' or ']' after array element",
                    ))
                }
            }
        }
    }
}

/// WTF-8 to UTF-16 code units (a lone surrogate stays a lone unit).
fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        let (width, initial) = match lead {
            0x00..=0x7F => (1, u32::from(lead)),
            0xC0..=0xDF => (2, u32::from(lead & 0x1F)),
            0xE0..=0xEF => (3, u32::from(lead & 0x0F)),
            _ => (4, u32::from(lead & 0x07)),
        };
        let mut code = initial;
        for k in 1..width {
            code = (code << 6) | u32::from(bytes.get(i + k).copied().unwrap_or(0x80) & 0x3F);
        }
        i += width;
        if code > 0xFFFF {
            let v = code - 0x1_0000;
            units.push(0xD800 | (v >> 10) as u16);
            units.push(0xDC00 | (v & 0x3FF) as u16);
        } else {
            units.push(code as u16);
        }
    }
    units
}

/// UTF-16 code units to WTF-8 (an unpaired surrogate is encoded alone).
fn push_wtf8(out: &mut Vec<u8>, units: &[u16]) {
    let mut i = 0;
    while i < units.len() {
        let unit = u32::from(units[i]);
        let code = if (0xD800..0xDC00).contains(&unit)
            && units
                .get(i + 1)
                .is_some_and(|&low| (0xDC00..0xE000).contains(&low))
        {
            i += 1;
            0x1_0000 + ((unit - 0xD800) << 10) + (u32::from(units[i]) - 0xDC00)
        } else {
            unit
        };
        i += 1;
        match code {
            0..=0x7F => out.push(code as u8),
            0x80..=0x7FF => out.extend([0xC0 | (code >> 6) as u8, 0x80 | (code & 0x3F) as u8]),
            0x800..=0xFFFF => out.extend([
                0xE0 | (code >> 12) as u8,
                0x80 | ((code >> 6) & 0x3F) as u8,
                0x80 | (code & 0x3F) as u8,
            ]),
            _ => out.extend([
                0xF0 | (code >> 18) as u8,
                0x80 | ((code >> 12) & 0x3F) as u8,
                0x80 | ((code >> 6) & 0x3F) as u8,
                0x80 | (code & 0x3F) as u8,
            ]),
        }
    }
}

/// V8's CalculateFileLocation: 1-based line and column; CR, LF and CRLF each
/// end one line.
fn location(units: &[u16], pos: usize) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    let mut i = 0;
    while i < pos {
        match units[i] {
            0x0D => {
                if units.get(i + 1) == Some(&0x0A) && i + 1 < pos {
                    i += 1;
                }
                line += 1;
                line_start = i + 1;
            }
            0x0A => {
                line += 1;
                line_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    (line, pos - line_start + 1)
}

/// The SyntaxError message V8 reports for the source `bytes` (WTF-8), or
/// `None` when the source is valid JSON by V8's grammar.
pub(super) fn v8_parse_error_message(bytes: &[u8]) -> Option<Vec<u8>> {
    let units = utf16_units(bytes);
    let mut scanner = Scanner {
        units: &units,
        pos: 0,
    };
    let failure = scanner.text().err()?;
    let mut message = Vec::new();
    let at = |message: &mut Vec<u8>, text: &str, pos: usize| {
        let (line, column) = location(&units, pos);
        message.extend_from_slice(
            // Every template but the trailing-data one reads "... in JSON at".
            format!(
                "{text}{} at position {pos} (line {line} column {column})",
                if text.ends_with("after JSON") {
                    ""
                } else {
                    " in JSON"
                }
            )
            .as_bytes(),
        );
    };
    match failure {
        Failure::End => message.extend_from_slice(b"Unexpected end of JSON input"),
        Failure::Number(pos) => at(&mut message, "Unexpected number", pos),
        Failure::At(pos, text) => at(&mut message, text, pos),
        Failure::Token(pos) => {
            let whole = &units[..];
            if [&b"NaN"[..], b"Infinity", b"undefined", b"[object Object]"]
                .iter()
                .any(|special| {
                    special.len() == whole.len()
                        && special.iter().zip(whole).all(|(&b, &u)| u16::from(b) == u)
                })
            {
                message.push(b'"');
                push_wtf8(&mut message, whole);
                message.extend_from_slice(b"\" is not valid JSON");
                return Some(message);
            }
            message.extend_from_slice(b"Unexpected token '");
            push_wtf8(&mut message, &units[pos..pos + 1]);
            message.extend_from_slice(b"', ");
            let length = units.len();
            if length <= SHORT_SOURCE {
                message.push(b'"');
                push_wtf8(&mut message, whole);
                message.push(b'"');
            } else if pos < CONTEXT {
                message.push(b'"');
                push_wtf8(&mut message, &units[..pos + CONTEXT]);
                message.extend_from_slice(b"\"...");
            } else if pos < length - CONTEXT {
                message.extend_from_slice(b"...\"");
                push_wtf8(&mut message, &units[pos - CONTEXT..pos + CONTEXT]);
                message.extend_from_slice(b"\"...");
            } else {
                message.extend_from_slice(b"...\"");
                push_wtf8(&mut message, &units[pos - CONTEXT..]);
                message.push(b'"');
            }
            message.extend_from_slice(b" is not valid JSON");
        }
    }
    Some(message)
}

#[cfg(test)]
#[path = "parse_error_tests.rs"]
mod tests;
