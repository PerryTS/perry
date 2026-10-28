//! Dependency-free JSON transport for the runtime build. Typed schema checks
//! live in the compiler consumer; adding serde to this crate would change the
//! runtime build dependency graph and therefore Cargo's runtime symbol ids.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }
    pub fn string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
    pub fn array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }
    pub fn encode(&self) -> String {
        fn quoted(value: &str, out: &mut String) {
            use std::fmt::Write;
            out.push('"');
            for c in value.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c < ' ' => {
                        write!(out, "\\u{:04x}", c as u32).unwrap();
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        fn write(value: &Value, out: &mut String) {
            match value {
                Value::Null => out.push_str("null"),
                Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
                Value::Number(v) => out.push_str(v),
                Value::String(v) => quoted(v, out),
                Value::Array(values) => {
                    out.push('[');
                    for (i, value) in values.iter().enumerate() {
                        if i != 0 {
                            out.push(',');
                        }
                        write(value, out);
                    }
                    out.push(']');
                }
                Value::Object(values) => {
                    out.push('{');
                    for (i, (key, value)) in values.iter().enumerate() {
                        if i != 0 {
                            out.push(',');
                        }
                        quoted(key, out);
                        out.push(':');
                        write(value, out);
                    }
                    out.push('}');
                }
            }
        }
        let mut out = String::new();
        write(self, &mut out);
        out
    }
}

pub fn parse(source: &str) -> Result<Value, String> {
    struct Parser<'a> {
        source: &'a str,
        index: usize,
    }
    impl Parser<'_> {
        fn error(&self) -> String {
            format!("invalid JSON at byte {}", self.index)
        }
        fn space(&mut self) {
            while self
                .source
                .as_bytes()
                .get(self.index)
                .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            {
                self.index += 1;
            }
        }
        fn take(&mut self, token: &str) -> bool {
            self.space();
            if self.source[self.index..].starts_with(token) {
                self.index += token.len();
                true
            } else {
                false
            }
        }
        fn hex(&mut self) -> Result<u32, String> {
            let end = self.index + 4;
            let s = self
                .source
                .get(self.index..end)
                .ok_or_else(|| self.error())?;
            let value = u32::from_str_radix(s, 16).map_err(|_| self.error())?;
            self.index = end;
            Ok(value)
        }
        fn string(&mut self) -> Result<String, String> {
            if !self.take("\"") {
                return Err(self.error());
            }
            let mut out = String::new();
            loop {
                let c = self.source[self.index..]
                    .chars()
                    .next()
                    .ok_or_else(|| self.error())?;
                self.index += c.len_utf8();
                match c {
                    '"' => return Ok(out),
                    '\\' => {
                        let escape = self
                            .source
                            .as_bytes()
                            .get(self.index)
                            .copied()
                            .ok_or_else(|| self.error())?;
                        self.index += 1;
                        out.push(match escape {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\x08',
                            b'f' => '\x0c',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let mut scalar = self.hex()?;
                                if (0xd800..=0xdbff).contains(&scalar) {
                                    if !self.source[self.index..].starts_with("\\u") {
                                        return Err(self.error());
                                    }
                                    self.index += 2;
                                    let low = self.hex()?;
                                    if !(0xdc00..=0xdfff).contains(&low) {
                                        return Err(self.error());
                                    }
                                    scalar = 0x10000 + ((scalar - 0xd800) << 10) + (low - 0xdc00);
                                }
                                char::from_u32(scalar).ok_or_else(|| self.error())?
                            }
                            _ => return Err(self.error()),
                        });
                    }
                    c if c < ' ' => return Err(self.error()),
                    c => out.push(c),
                }
            }
        }
        fn value(&mut self) -> Result<Value, String> {
            self.space();
            match self.source.as_bytes().get(self.index).copied() {
                Some(b'"') => Ok(Value::String(self.string()?)),
                Some(b'{') => {
                    self.index += 1;
                    let mut map = BTreeMap::new();
                    if !self.take("}") {
                        loop {
                            let key = self.string()?;
                            if !self.take(":") {
                                return Err(self.error());
                            }
                            if map.insert(key, self.value()?).is_some() {
                                return Err("duplicate JSON key".into());
                            }
                            if self.take("}") {
                                break;
                            }
                            if !self.take(",") {
                                return Err(self.error());
                            }
                        }
                    }
                    Ok(Value::Object(map))
                }
                Some(b'[') => {
                    self.index += 1;
                    let mut values = Vec::new();
                    if !self.take("]") {
                        loop {
                            values.push(self.value()?);
                            if self.take("]") {
                                break;
                            }
                            if !self.take(",") {
                                return Err(self.error());
                            }
                        }
                    }
                    Ok(Value::Array(values))
                }
                Some(b'n') if self.take("null") => Ok(Value::Null),
                Some(b't') if self.take("true") => Ok(Value::Bool(true)),
                Some(b'f') if self.take("false") => Ok(Value::Bool(false)),
                Some(b'-' | b'0'..=b'9') => {
                    let start = self.index;
                    if self.source.as_bytes().get(self.index) == Some(&b'-') {
                        self.index += 1;
                    }
                    let digits = |this: &mut Self| {
                        let start = this.index;
                        while this
                            .source
                            .as_bytes()
                            .get(this.index)
                            .is_some_and(u8::is_ascii_digit)
                        {
                            this.index += 1;
                        }
                        this.index > start
                    };
                    if self.source.as_bytes().get(self.index) == Some(&b'0') {
                        self.index += 1;
                    } else if !self
                        .source
                        .as_bytes()
                        .get(self.index)
                        .is_some_and(|b| matches!(b, b'1'..=b'9'))
                        || !digits(self)
                    {
                        return Err(self.error());
                    }
                    if self.source.as_bytes().get(self.index) == Some(&b'.') {
                        self.index += 1;
                        if !digits(self) {
                            return Err(self.error());
                        }
                    }
                    if self
                        .source
                        .as_bytes()
                        .get(self.index)
                        .is_some_and(|b| matches!(b, b'e' | b'E'))
                    {
                        self.index += 1;
                        if self
                            .source
                            .as_bytes()
                            .get(self.index)
                            .is_some_and(|b| matches!(b, b'+' | b'-'))
                        {
                            self.index += 1;
                        }
                        if !digits(self) {
                            return Err(self.error());
                        }
                    }
                    Ok(Value::Number(self.source[start..self.index].into()))
                }
                _ => Err(self.error()),
            }
        }
    }
    let mut p = Parser { source, index: 0 };
    let value = p.value()?;
    p.space();
    if p.index != source.len() {
        return Err(p.error());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_unicode() {
        let v = parse(r#"{"n":-12.5e+3,"a":[null,true,false,"\uD83D\uDE00\n\u0000"]}"#).unwrap();
        assert_eq!(parse(&v.encode()).unwrap(), v);
        assert_eq!(
            v.get("a").unwrap().array().unwrap()[3].string(),
            Some("😀\n\0")
        );
    }
    #[test]
    fn malformed_transport_fails_closed() {
        for source in [
            "01",
            "- 1",
            "1.",
            "1e",
            "-",
            "[1,]",
            "{\"a\":1,\"a\":2}",
            "\"\\uD800\"",
            "\"\\uDC00\"",
            "[true false]",
            "null trailing",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}
