//! The `PERRY_PROCESS_EXEC_ARGV` wire value: a JSON array of strings that a
//! forking parent writes (`child_process::fork`, `serde_json::to_string`).
//!
//! `process.execArgv` is reachable in every program, so its reader must not
//! link a general JSON deserializer: this decoder accepts exactly what
//! `serde_json::from_str::<Vec<String>>` accepts and yields the same strings
//! (`None` where serde_json reports an error).

/// Decode `raw` as a JSON array of strings.
pub(super) fn decode(raw: &str) -> Option<Vec<String>> {
    let mut p = Parser {
        s: raw.as_bytes(),
        i: 0,
    };
    p.ws();
    p.eat(b'[')?;
    let mut out = Vec::new();
    p.ws();
    if p.peek() == Some(b']') {
        p.i += 1;
    } else {
        loop {
            p.ws();
            out.push(p.string()?);
            p.ws();
            match p.next()? {
                b',' => continue,
                b']' => break,
                _ => return None,
            }
        }
    }
    p.ws();
    (p.i == p.s.len()).then_some(out)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.i += 1;
        Some(b)
    }

    fn eat(&mut self, want: u8) -> Option<()> {
        (self.next()? == want).then_some(())
    }

    /// JSON whitespace (RFC 8259: space, tab, line feed, carriage return).
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            v = v * 16 + (self.next()? as char).to_digit(16)?;
        }
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            match self.next()? {
                b'"' => break,
                b'\\' => {
                    let c = match self.next()? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = match hi {
                                0xD800..=0xDBFF => {
                                    self.eat(b'\\')?;
                                    self.eat(b'u')?;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&lo) {
                                        return None;
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                }
                                0xDC00..=0xDFFF => return None,
                                _ => hi,
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                // A raw control character is not allowed inside a string.
                b if b < 0x20 => return None,
                b => out.push(b),
            }
        }
        // The input is a `str` and every escape wrote a whole character.
        String::from_utf8(out).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    fn serde(raw: &str) -> Option<Vec<String>> {
        serde_json::from_str::<Vec<String>>(raw).ok()
    }

    /// The decoder and serde_json agree on every input of the corpus, the
    /// accepted ones (with their strings) and the rejected ones.
    #[test]
    fn decodes_exactly_what_serde_json_decodes() {
        let corpus = [
            "[]",
            " [ ] ",
            "\n[\t]\r\n",
            "[\"--inspect\"]",
            "[\"--max-old-space-size=64\",\"--expose-gc\"]",
            "[ \"a\" , \"b\" ]",
            "[\"\"]",
            "[\"q\\\"uote\",\"back\\\\slash\",\"sl\\/ash\"]",
            "[\"\\b\\f\\n\\r\\t\"]",
            "[\"\\u0041\\u00e9\\u20AC\"]",
            "[\"\\ud83d\\ude00\"]",
            "[\"\\uD83D\\uDE00x\"]",
            "[\"caf\u{e9} \u{1f600}\"]",
            "[\"\\u0000\"]",
            "[\"\\ud83d\"]",
            "[\"\\ude00\"]",
            "[\"\\ud83d\\u0041\"]",
            "[\"\\ud83dx\"]",
            "[\"\\u12\"]",
            "[\"\\u12G4\"]",
            "[\"\\x\"]",
            "[\"tab\there\"]",
            "[\"nl\nhere\"]",
            "[\"a\",]",
            "[,\"a\"]",
            "[\"a\" \"b\"]",
            "[\"a\"",
            "[\"a",
            "[1]",
            "[\"a\",null]",
            "[[\"a\"]]",
            "{}",
            "\"a\"",
            "",
            " ",
            "[] x",
            "[]]",
            "[\"a\"]\u{a0}",
            "\u{feff}[]",
        ];
        for raw in corpus {
            assert_eq!(decode(raw), serde(raw), "input {raw:?}");
        }
    }

    /// What the forking parent writes comes back unchanged.
    #[test]
    fn round_trips_the_fork_encoding() {
        let args: Vec<String> = vec![
            "--inspect=127.0.0.1:9229".into(),
            "quote \" back \\ slash / ".into(),
            "\u{1}\u{1f}\u{7f} control".into(),
            "caf\u{e9} \u{1f600} \u{2028}".into(),
            String::new(),
        ];
        let wire = serde_json::to_string(&args).unwrap();
        assert_eq!(decode(&wire), Some(args));
    }
}
