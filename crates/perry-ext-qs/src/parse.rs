use crate::codec::{self, Charset};
use crate::options::{DuplicateMode, ParseOptions};
use perry_ffi::{throw_with_code, ErrorKind};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Key(String),
    Index(usize),
    Append,
}

pub(crate) fn parse(input: &str, options: &mut ParseOptions) -> Value {
    let input = if options.ignore_query_prefix {
        input.strip_prefix('?').unwrap_or(input)
    } else {
        input
    };
    if input.is_empty() {
        return Value::Object(Map::new());
    }

    let mut pairs: Vec<&str> = if options.delimiter.is_empty() {
        vec![input]
    } else {
        input.split(&options.delimiter).collect()
    };
    if pairs.len() > options.parameter_limit {
        if options.throw_on_limit_exceeded {
            throw_with_code(
                &format!(
                    "Parameter limit exceeded. Only {} parameter{} allowed.",
                    options.parameter_limit,
                    if options.parameter_limit == 1 {
                        " is"
                    } else {
                        "s are"
                    }
                ),
                "",
                ErrorKind::RangeError,
            );
        }
        pairs.truncate(options.parameter_limit);
    }

    if options.charset_sentinel {
        if let Some((index, charset)) = pairs.iter().enumerate().find_map(|(index, pair)| {
            if *pair == "utf8=%E2%9C%93" {
                Some((index, Charset::Utf8))
            } else if *pair == "utf8=%26%2310003%3B" {
                Some((index, Charset::Latin1))
            } else {
                None
            }
        }) {
            options.charset = charset;
            pairs.remove(index);
        }
    }

    let mut root = Value::Object(Map::new());
    for pair in pairs {
        let (raw_key, raw_value, had_equals) = match pair.find('=') {
            Some(index) => (&pair[..index], &pair[index + 1..], true),
            None => (pair, "", false),
        };
        let mut key = codec::decode(raw_key, options.charset);
        if options.decode_dot_in_keys {
            key = key.replace("%2E", ".").replace("%2e", ".");
        }
        let mut value = codec::decode(raw_value, options.charset);
        if options.charset == Charset::Latin1 && options.interpret_numeric_entities {
            value = decode_numeric_entities(&value);
        }

        let mut segments = parse_segments(&key, options);
        if segments.is_empty() || forbidden_path(&segments, options.allow_prototypes) {
            continue;
        }

        let empty_array = !had_equals
            && options.allow_empty_arrays
            && matches!(segments.last(), Some(Segment::Append));
        let parsed_value = if empty_array {
            segments.pop();
            Value::Array(Vec::new())
        } else if !had_equals && options.strict_null_handling {
            Value::Null
        } else if options.comma && value.contains(',') {
            Value::Array(
                value
                    .split(',')
                    .map(|part| Value::String(part.to_owned()))
                    .collect(),
            )
        } else {
            Value::String(value)
        };
        insert(&mut root, &segments, parsed_value, options);
    }
    root
}

fn parse_segments(key: &str, options: &ParseOptions) -> Vec<Segment> {
    let use_dots = options.allow_dots || options.decode_dot_in_keys;
    let mut raw_segments = Vec::new();
    let mut index = key
        .char_indices()
        .find_map(|(index, ch)| (ch == '[' || (use_dots && ch == '.')).then_some(index))
        .unwrap_or(key.len());
    raw_segments.push(key[..index].to_owned());
    let mut nested = 0usize;

    while index < key.len() {
        match key.as_bytes()[index] {
            b'.' if use_dots => {
                let start = index + 1;
                let end = key[start..]
                    .char_indices()
                    .find_map(|(offset, ch)| {
                        (ch == '[' || (use_dots && ch == '.')).then_some(start + offset)
                    })
                    .unwrap_or(key.len());
                raw_segments.push(key[start..end].to_owned());
                index = end;
            }
            b'[' => {
                let bracket_start = index;
                let Some(close_offset) = key[index + 1..].find(']') else {
                    raw_segments.push(key[index..].to_owned());
                    break;
                };
                let close = index + 1 + close_offset;
                nested += 1;
                if nested > options.depth {
                    if options.strict_depth {
                        throw_with_code(
                            &format!(
                                "Input depth exceeded depth option of {} and strictDepth is true",
                                options.depth
                            ),
                            "",
                            ErrorKind::RangeError,
                        );
                    }
                    raw_segments.push(key[bracket_start..].to_owned());
                    index = key.len();
                    continue;
                }
                raw_segments.push(key[index + 1..close].to_owned());
                index = close + 1;
            }
            _ => {
                raw_segments.push(key[index..].to_owned());
                break;
            }
        }
    }

    raw_segments
        .into_iter()
        .enumerate()
        .map(|(position, segment)| {
            if position > 0 && segment.is_empty() && options.parse_arrays {
                Segment::Append
            } else if position > 0 && options.parse_arrays {
                match segment.parse::<usize>() {
                    Ok(index) if index <= options.array_limit => Segment::Index(index),
                    _ => Segment::Key(segment),
                }
            } else {
                Segment::Key(segment)
            }
        })
        .collect()
}

fn forbidden_path(segments: &[Segment], allow_prototypes: bool) -> bool {
    const OBJECT_PROTOTYPE_KEYS: &[&str] = &[
        "__defineGetter__",
        "__defineSetter__",
        "__lookupGetter__",
        "__lookupSetter__",
        "constructor",
        "hasOwnProperty",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "toLocaleString",
        "toString",
        "valueOf",
    ];
    segments.iter().any(|segment| match segment {
        Segment::Key(key) if key == "__proto__" => true,
        Segment::Key(key) if !allow_prototypes => OBJECT_PROTOTYPE_KEYS.contains(&key.as_str()),
        _ => false,
    })
}

fn insert(node: &mut Value, segments: &[Segment], value: Value, options: &ParseOptions) {
    let Some((segment, rest)) = segments.split_first() else {
        merge_leaf(node, value, options.duplicates);
        return;
    };

    match segment {
        Segment::Key(key) => {
            if !node.is_object() {
                *node = Value::Object(Map::new());
            }
            let object = node.as_object_mut().expect("object initialized");
            if rest.is_empty() {
                match object.get_mut(key) {
                    Some(existing) => merge_leaf(existing, value, options.duplicates),
                    None => {
                        object.insert(key.clone(), value);
                    }
                }
                return;
            }
            let child = object
                .entry(key.clone())
                .or_insert_with(|| empty_container(&rest[0], options));
            insert(child, rest, value, options);
        }
        Segment::Index(requested) => {
            if !node.is_array() {
                *node = Value::Array(Vec::new());
            }
            let array = node.as_array_mut().expect("array initialized");
            let position = if options.allow_sparse {
                while array.len() <= *requested {
                    array.push(Value::Null);
                }
                *requested
            } else if *requested < array.len() {
                *requested
            } else {
                if rest.is_empty() {
                    array.push(value);
                    return;
                }
                array.push(empty_container(&rest[0], options));
                let position = array.len() - 1;
                insert(&mut array[position], rest, value, options);
                return;
            };
            if rest.is_empty() {
                if options.allow_sparse && array[position].is_null() {
                    array[position] = value;
                } else {
                    merge_leaf(&mut array[position], value, options.duplicates);
                }
            } else {
                if array[position].is_null() {
                    array[position] = empty_container(&rest[0], options);
                }
                insert(&mut array[position], rest, value, options);
            }
        }
        Segment::Append => {
            if !node.is_array() {
                *node = Value::Array(Vec::new());
            }
            let array = node.as_array_mut().expect("array initialized");
            if rest.is_empty() {
                array.push(value);
            } else {
                let mut child = empty_container(&rest[0], options);
                insert(&mut child, rest, value, options);
                array.push(child);
            }
        }
    }
}

fn empty_container(next: &Segment, options: &ParseOptions) -> Value {
    if options.parse_arrays && matches!(next, Segment::Index(_) | Segment::Append) {
        Value::Array(Vec::new())
    } else {
        Value::Object(Map::new())
    }
}

fn merge_leaf(existing: &mut Value, value: Value, mode: DuplicateMode) {
    match mode {
        DuplicateMode::First => {}
        DuplicateMode::Last => *existing = value,
        DuplicateMode::Combine => match existing {
            Value::Array(values) => values.push(value),
            _ => {
                let previous = std::mem::replace(existing, Value::Null);
                *existing = Value::Array(vec![previous, value]);
            }
        },
    }
}

fn decode_numeric_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("&#") {
        output.push_str(&rest[..start]);
        let entity = &rest[start + 2..];
        let Some(end) = entity.find(';') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let digits = &entity[..end];
        if let Ok(codepoint) = digits.parse::<u32>() {
            if let Some(ch) = char::from_u32(codepoint) {
                output.push(ch);
            } else {
                output.push_str(&rest[start..start + end + 3]);
            }
        } else {
            output.push_str(&rest[start..start + end + 3]);
        }
        rest = &entity[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> Value {
        parse(input, &mut ParseOptions::default())
    }

    #[test]
    fn parses_nested_objects_arrays_and_duplicates() {
        assert_eq!(
            parsed("customer[name]=Ada&items[0][id]=price_1&items[1][id]=price_2&tag=a&tag=b"),
            serde_json::json!({
                "customer": { "name": "Ada" },
                "items": [{ "id": "price_1" }, { "id": "price_2" }],
                "tag": ["a", "b"]
            })
        );
    }

    #[test]
    fn blocks_prototype_pollution_segments() {
        assert_eq!(
            parsed("safe=yes&__proto__[polluted]=yes&constructor[prototype][bad]=yes"),
            serde_json::json!({ "safe": "yes" })
        );
    }

    #[test]
    fn array_limit_falls_back_to_object_key() {
        assert_eq!(parsed("a[21]=x"), serde_json::json!({ "a": { "21": "x" } }));
    }

    #[test]
    fn supports_dot_and_sparse_modes() {
        let mut options = ParseOptions {
            allow_dots: true,
            allow_sparse: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("a.b[2]=x", &mut options),
            serde_json::json!({ "a": { "b": [null, null, "x"] } })
        );
    }

    #[test]
    fn allow_empty_arrays_matches_qs_without_strict_null_mode() {
        let mut options = ParseOptions {
            allow_empty_arrays: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("foo[]", &mut options),
            serde_json::json!({ "foo": [] })
        );
    }

    #[test]
    fn duplicates_modes_match_qs() {
        let mut first = ParseOptions {
            duplicates: DuplicateMode::First,
            ..ParseOptions::default()
        };
        let mut last = ParseOptions {
            duplicates: DuplicateMode::Last,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("a=b&a=c", &mut first),
            serde_json::json!({ "a": "b" })
        );
        assert_eq!(parse("a=b&a=c", &mut last), serde_json::json!({ "a": "c" }));
    }

    #[test]
    fn query_prefix_comma_and_strict_null_options_match_qs() {
        let mut comma = ParseOptions {
            ignore_query_prefix: true,
            comma: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("?a=b,c", &mut comma),
            serde_json::json!({ "a": ["b", "c"] })
        );

        let mut strict = ParseOptions {
            strict_null_handling: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("a&b=", &mut strict),
            serde_json::json!({ "a": null, "b": "" })
        );
    }

    #[test]
    fn charset_sentinel_depth_and_encoded_dots_match_qs() {
        let mut charset = ParseOptions {
            charset_sentinel: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("utf8=%26%2310003%3B&a=%F8", &mut charset),
            serde_json::json!({ "a": "ø" })
        );

        assert_eq!(
            parsed("a[b][c][d][e][f][g]=h"),
            serde_json::json!({
                "a": { "b": { "c": { "d": { "e": { "f": { "[g]": "h" } } } } } }
            })
        );

        let mut dots = ParseOptions {
            decode_dot_in_keys: true,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse("a%2Eb=c", &mut dots),
            serde_json::json!({ "a": { "b": "c" } })
        );
    }
}
