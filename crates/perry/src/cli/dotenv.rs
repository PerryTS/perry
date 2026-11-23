//! Project-local environment loading for publish. Existing process values win.
//! This grammar includes interpolation; Node util.parseEnv intentionally does
//! not, so it remains a separate parser.
use std::{collections::HashMap, io, path::Path};

pub fn from_path(path: &Path) -> io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut values = HashMap::new();
    for record in records(text.trim_start_matches('\u{feff}')) {
        let Some((key, value)) = parse_record(record, &|name| {
            std::env::var(name)
                .ok()
                .or_else(|| values.get(name).cloned())
        })?
        else {
            continue;
        };
        if std::env::var_os(&key).is_none() {
            std::env::set_var(&key, &value);
        }
        values.insert(key, value);
    }
    Ok(())
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid .env assignment")
}
fn records(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    for (i, c) in text.char_indices() {
        if c == '\n' && (quote.is_none() || comment) {
            out.push(&text[start..i]);
            start = i + 1;
            comment = false;
            escaped = false;
            continue;
        }
        if comment {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            quote = None;
        } else if quote.is_none() && matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if quote.is_none() && c == '#' {
            comment = true;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}
fn parse_record(
    line: &str,
    lookup: &impl Fn(&str) -> Option<String>,
) -> io::Result<Option<(String, String)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let line = line
        .strip_prefix("export ")
        .map(str::trim_start)
        .unwrap_or(line);
    let (key, value) = line.split_once('=').ok_or_else(invalid)?;
    let key = key.trim();
    if key.is_empty()
        || !key.chars().enumerate().all(|(i, c)| {
            c == '_' || c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || c == '.'))
        })
    {
        return Err(invalid());
    }
    let chars: Vec<_> = value.trim_start().chars().collect();
    let mut out = String::new();
    let mut quote = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if quote == Some(c) {
            quote = None;
            continue;
        }
        if quote.is_none() && matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if quote.is_none() && c.is_whitespace() {
            if chars[i..]
                .iter()
                .find(|c| !c.is_whitespace())
                .is_some_and(|c| *c != '#')
            {
                return Err(invalid());
            }
            break;
        }
        if c == '\\' && quote != Some('\'') {
            let next = *chars.get(i).ok_or_else(invalid)?;
            i += 1;
            out.push(match next {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                '\'' => '\'',
                '$' => '$',
                ' ' => ' ',
                _ => return Err(invalid()),
            });
        } else if c == '$' && quote != Some('\'') {
            let braced = chars.get(i) == Some(&'{');
            if braced {
                i += 1;
            }
            let start = i;
            while chars
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if braced {
                if chars.get(i) != Some(&'}') {
                    return Err(invalid());
                }
                i += 1;
            }
            out.push_str(&lookup(&name).unwrap_or_default());
        } else {
            out.push(c);
        }
    }
    if quote.is_some() || out.contains('\0') {
        return Err(invalid());
    }
    Ok(Some((key.to_string(), out)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_multiline_and_substitution() {
        let lookup = |name: &str| (name == "KEY").then(|| "secret".to_string());
        for (input, expected) in [
            ("A=${KEY}/$KEY", "secret/secret"),
            ("A='$KEY'", "$KEY"),
            ("A=\"line\\n$KEY\"", "line\nsecret"),
            ("export A=value # ignored", "value"),
            ("A=foo#bar", "foo#bar"),
        ] {
            assert_eq!(parse_record(input, &lookup).unwrap().unwrap().1, expected);
        }
        assert_eq!(records("A='one\ntwo'\n# comment\nB=3").len(), 3);
        assert!(parse_record("A=${KEY", &lookup).is_err());
        assert!(parse_record("1A=x", &lookup).is_err());
    }
}

#[cfg(test)]
mod compatibility {
    use super::*;
    #[test]
    fn publish_environment_grammar_matches_existing_loader() {
        for input in [
            "A=plain",
            "A=",
            "A=  value  # comment",
            "export A=one",
            "A='two words'",
            "A=\"line\\nnext\"",
            "A='one\ntwo'",
            "A=foo#literal",
            "A=foo\\ bar",
            "A='a\"b'",
            "A=\"a'b\"",
            "A=${PERRY_LEAN_UNSET_TEST_VARIABLE}",
            "A=\"${PERRY_LEAN_UNSET_TEST_VARIABLE}\"",
            "A='${PERRY_LEAN_UNSET_TEST_VARIABLE}'",
            "A=\"\\$literal\"",
            "# comment\nA=42\n",
            "A=\"unterminated",
            "1A=invalid",
            "A=bad\\q",
        ] {
            let reference: Option<Vec<_>> = dotenvy::from_read_iter(input.as_bytes())
                .collect::<Result<Vec<_>, _>>()
                .ok();
            let ours: Option<Vec<_>> = records(input)
                .into_iter()
                .map(|r| parse_record(r, &|_| None))
                .collect::<io::Result<Vec<_>>>()
                .ok()
                .map(|pairs| pairs.into_iter().flatten().collect());
            assert_eq!(ours, reference, "{input:?}");
        }
    }
}
