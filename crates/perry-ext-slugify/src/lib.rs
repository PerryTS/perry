//! Native bindings for the npm `slugify` package.
//!
//! Functionally identical to `crates/perry-stdlib/src/slugify.rs` —
//! both follow simov/slugify's actual algorithm:
//!
//! 1. per-char charMap substitution (case-preserving: 'É' → 'E');
//! 2. a mapped char equal to `options.replacement` becomes a space;
//! 3. chars outside the default keep-set `[\w\s$*_+~.()'"!\-:@]` are
//!    removed (the `remove` regex option is not supported);
//! 4. `strict` strips everything but `[A-Za-z0-9\s]`;
//! 5. `trim` (default true) trims whitespace;
//! 6. whitespace runs collapse to the (full, possibly multi-char)
//!    replacement string;
//! 7. `lower` lowercases the final slug.
//!
//! The second argument mirrors npm slugify's overloads: a plain string
//! (the replacement) or an options object `{ replacement, lower,
//! strict, trim }`. It crosses the FFI as raw NaN-box bits (i64) so
//! this wrapper can distinguish string / object / undefined — the old
//! coerce-to-string ABI is what garbled `slugify(s, { lower: true })`
//! into `hello{world` (the JSON-stringified object's first char `{`
//! became the separator).
//!
//! Depends only on [`perry_ffi`] plus three C-ABI runtime symbols
//! (declared below, resolved at final link — the perry-ext-events
//! pattern for by-name object field reads).

use perry_ffi::{alloc_string, read_string, JsString, JsValue, ObjectHeader, StringHeader};

extern "C" {
    /// perry-runtime: read an object field by string key, returning the
    /// raw NaN-boxed JSValue bits as f64 (undefined tag when absent).
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    /// perry-runtime: JS truthiness probe for a NaN-boxed value.
    fn js_is_truthy(value: f64) -> i32;
    /// perry-runtime: extract the StringHeader pointer from any
    /// string-tagged NaN-boxed value.
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Subset of npm slugify's charMap. Case-preserving, may expand to
/// multiple chars ('ß' → "ss", '&' → "and") — exactly like the npm map.
fn char_map(c: char) -> Option<&'static str> {
    Some(match c {
        'À' => "A",
        'Á' => "A",
        'Â' => "A",
        'Ã' => "A",
        'Ä' => "A",
        'Å' => "A",
        'Æ' => "AE",
        'Ç' => "C",
        'È' => "E",
        'É' => "E",
        'Ê' => "E",
        'Ë' => "E",
        'Ì' => "I",
        'Í' => "I",
        'Î' => "I",
        'Ï' => "I",
        'Ð' => "D",
        'Ñ' => "N",
        'Ò' => "O",
        'Ó' => "O",
        'Ô' => "O",
        'Õ' => "O",
        'Ö' => "O",
        'Ø' => "O",
        'Ù' => "U",
        'Ú' => "U",
        'Û' => "U",
        'Ü' => "U",
        'Ý' => "Y",
        'Þ' => "TH",
        'ß' => "ss",
        'à' => "a",
        'á' => "a",
        'â' => "a",
        'ã' => "a",
        'ä' => "a",
        'å' => "a",
        'æ' => "ae",
        'ç' => "c",
        'è' => "e",
        'é' => "e",
        'ê' => "e",
        'ë' => "e",
        'ì' => "i",
        'í' => "i",
        'î' => "i",
        'ï' => "i",
        'ð' => "d",
        'ñ' => "n",
        'ò' => "o",
        'ó' => "o",
        'ô' => "o",
        'õ' => "o",
        'ö' => "o",
        'ø' => "o",
        'ù' => "u",
        'ú' => "u",
        'û' => "u",
        'ü' => "u",
        'ý' => "y",
        'þ' => "th",
        'ÿ' => "y",
        'Ÿ' => "Y",
        'Œ' => "OE",
        'œ' => "oe",
        '&' => "and",
        '|' => "or",
        '<' => "less",
        '>' => "greater",
        '©' => "(c)",
        '®' => "(r)",
        '™' => "tm",
        _ => return None,
    })
}

/// JS `\s` (non-unicode regex flag): ASCII whitespace + the Unicode
/// space separators the npm regexes match.
fn js_space_char(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn default_keep(c: char) -> bool {
    // JS `[\w\s$*_+~.()'"!\-:@]` with the default (ASCII) `\w`.
    c.is_ascii_alphanumeric()
        || c == '_'
        || js_space_char(c)
        || matches!(
            c,
            '$' | '*' | '+' | '~' | '.' | '(' | ')' | '\'' | '"' | '!' | '-' | ':' | '@'
        )
}

/// Parsed slugify options (npm surface; `remove` / `locale` unsupported).
struct SlugifyOptions {
    /// The replacement string for whitespace runs. npm defaults it to
    /// "-" BEFORE the per-char `appendChar === replacement` check, so a
    /// literal '-' in the input collapses with adjacent whitespace even
    /// when no replacement was supplied (slugify('a - b') === 'a-b').
    replacement: String,
    lower: bool,
    strict: bool,
    trim: bool,
}

impl Default for SlugifyOptions {
    fn default() -> Self {
        SlugifyOptions {
            replacement: "-".to_string(),
            lower: false,
            strict: false,
            trim: true,
        }
    }
}

/// Core algorithm — mirrors npm slugify's reduce + post-passes ordering.
fn slugify_npm(input: &str, opts: &SlugifyOptions) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut buf = [0u8; 4];
    for ch in input.chars() {
        let mapped: &str = match char_map(ch) {
            Some(m) => m,
            None => ch.encode_utf8(&mut buf),
        };
        // `if (appendChar === replacement) appendChar = ' '` — with the
        // already-defaulted replacement.
        let effective: &str = if mapped == opts.replacement {
            " "
        } else {
            mapped
        };
        for c in effective.chars() {
            if default_keep(c) {
                slug.push(c);
            }
        }
    }

    if opts.strict {
        slug.retain(|c| c.is_ascii_alphanumeric() || js_space_char(c));
    }
    if opts.trim {
        slug = slug.trim_matches(js_space_char).to_string();
    }

    // replace(/\s+/g, replacement)
    let mut out = String::with_capacity(slug.len());
    let mut in_ws = false;
    for c in slug.chars() {
        if js_space_char(c) {
            if !in_ws {
                out.push_str(&opts.replacement);
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }

    if opts.lower {
        out.to_lowercase()
    } else {
        out
    }
}

unsafe fn string_from_bits(bits: u64) -> Option<String> {
    let ptr = js_get_string_pointer_unified(f64::from_bits(bits)) as *mut StringHeader;
    if ptr.is_null() {
        return None;
    }
    read_string(JsString::from_raw(ptr)).map(String::from)
}

/// Decode the second slugify argument from raw NaN-box bits.
unsafe fn options_from_bits(options_bits: i64) -> SlugifyOptions {
    let mut opts = SlugifyOptions::default();
    let bits = options_bits as u64;
    let jv = JsValue::from_bits(bits);

    if jv.is_any_string() {
        if let Some(s) = string_from_bits(bits) {
            opts.replacement = s;
        }
        return opts;
    }

    if !jv.is_pointer() {
        return opts;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x100000 {
        return opts;
    }

    let field = |name: &str| -> f64 {
        let key = alloc_string(name);
        js_object_get_field_by_name_f64(obj, key.as_raw())
    };

    let replacement = field("replacement");
    if JsValue::from_bits(replacement.to_bits()).is_any_string() {
        if let Some(s) = string_from_bits(replacement.to_bits()) {
            opts.replacement = s;
        }
    }
    opts.lower = js_is_truthy(field("lower")) != 0;
    opts.strict = js_is_truthy(field("strict")) != 0;
    // npm: `if (options.trim !== false) slug = slug.trim()` — absent
    // (undefined) means trim.
    let trim = field("trim");
    opts.trim = JsValue::from_bits(trim.to_bits()).is_undefined() || js_is_truthy(trim) != 0;
    opts
}

/// `slugify(string)` — default slug with `-` separator (case-preserved,
/// matching npm).
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_slugify(input_ptr: *const StringHeader) -> *mut StringHeader {
    js_slugify_with_options(input_ptr, TAG_UNDEFINED as i64)
}

/// `slugify(string, replacementOrOptions)` — `options_bits` carries the
/// second JS argument as raw NaN-box bits: string → replacement, object
/// → `{ replacement, lower, strict, trim }`, undefined → defaults.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`;
/// `options_bits` must be valid NaN-box bits.
#[no_mangle]
pub unsafe extern "C" fn js_slugify_with_options(
    input_ptr: *const StringHeader,
    options_bits: i64,
) -> *mut StringHeader {
    let input_handle = JsString::from_raw(input_ptr as *mut StringHeader);
    let Some(input) = read_string(input_handle) else {
        return std::ptr::null_mut();
    };
    let opts = options_from_bits(options_bits);
    alloc_string(&slugify_npm(input, &opts)).as_raw()
}

/// `slugify(string, { strict: true })` — legacy strict entry point.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_slugify_strict(input_ptr: *const StringHeader) -> *mut StringHeader {
    let handle = JsString::from_raw(input_ptr as *mut StringHeader);
    let Some(input) = read_string(handle) else {
        return std::ptr::null_mut();
    };
    let opts = SlugifyOptions {
        strict: true,
        ..SlugifyOptions::default()
    };
    alloc_string(&slugify_npm(input, &opts)).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preserves_case_like_npm() {
        let opts = SlugifyOptions::default();
        assert_eq!(slugify_npm("Hello World", &opts), "Hello-World");
    }

    #[test]
    fn lower_option_lowercases() {
        let opts = SlugifyOptions {
            lower: true,
            ..SlugifyOptions::default()
        };
        assert_eq!(slugify_npm("Hello World", &opts), "hello-world");
    }

    #[test]
    fn string_replacement_is_full_string() {
        let opts = SlugifyOptions {
            replacement: "__".into(),
            ..SlugifyOptions::default()
        };
        assert_eq!(slugify_npm("foo bar", &opts), "foo__bar");
    }

    #[test]
    fn strict_removes_non_alnum() {
        let opts = SlugifyOptions {
            strict: true,
            lower: true,
            ..SlugifyOptions::default()
        };
        assert_eq!(
            slugify_npm("Hello, World! (2024)", &opts),
            "hello-world-2024"
        );
    }

    #[test]
    fn accents_fold_case_preserving() {
        let opts = SlugifyOptions::default();
        assert_eq!(slugify_npm("Crème Brûlée", &opts), "Creme-Brulee");
    }

    #[test]
    fn ampersand_maps_to_and() {
        let opts = SlugifyOptions {
            lower: true,
            ..SlugifyOptions::default()
        };
        assert_eq!(slugify_npm("Foo & Bar", &opts), "foo-and-bar");
    }

    #[test]
    fn default_dash_collapses_with_whitespace() {
        let opts = SlugifyOptions::default();
        assert_eq!(slugify_npm("a - b", &opts), "a-b");
        assert_eq!(slugify_npm("a-b", &opts), "a-b");
    }

    #[test]
    fn trim_false_keeps_edges_as_separators() {
        let opts = SlugifyOptions {
            trim: false,
            ..SlugifyOptions::default()
        };
        assert_eq!(slugify_npm(" foo bar ", &opts), "-foo-bar-");
    }
}
