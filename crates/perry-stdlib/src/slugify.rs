//! Slugify module (slugify compatible)
//!
//! Native implementation of the 'slugify' npm package (simov/slugify),
//! following the package's actual algorithm:
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
//! strict, trim }`. It crosses the FFI as raw NaN-box bits (i64) so the
//! runtime can distinguish string / object / undefined — passing it as
//! a coerced string is what garbled `slugify(s, { lower: true })` into
//! `hello{world` (the JSON-stringified object's first char `{` became
//! the separator).

use perry_runtime::{js_string_from_bytes, JSValue, ObjectHeader, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Subset of npm slugify's charMap. Case-preserving, may expand to
/// multiple chars ('ß' → "ss", '&' → "and") — exactly like the npm map.
pub(crate) fn char_map(c: char) -> Option<&'static str> {
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
pub(crate) fn js_space_char(c: char) -> bool {
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

/// Parsed slugify options (mirrors npm's option surface; `remove` and
/// `locale` are not supported).
pub(crate) struct SlugifyOptions {
    /// The replacement string for whitespace runs. npm defaults it to
    /// "-" BEFORE the per-char `appendChar === replacement` check, so a
    /// literal '-' in the input collapses with adjacent whitespace even
    /// when no replacement was supplied (slugify('a - b') === 'a-b').
    pub replacement: String,
    pub lower: bool,
    pub strict: bool,
    pub trim: bool,
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

/// Core algorithm shared by every entry point. Mirrors npm slugify's
/// reduce + post-passes ordering exactly.
pub(crate) fn slugify_npm(input: &str, opts: &SlugifyOptions) -> String {
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

/// Decode the second slugify argument from raw NaN-box bits:
/// string → `{ replacement }`, object → `{ replacement, lower, strict,
/// trim }`, anything else → defaults.
unsafe fn options_from_bits(options_bits: i64) -> SlugifyOptions {
    let mut opts = SlugifyOptions::default();
    let value = f64::from_bits(options_bits as u64);
    let jv = JSValue::from_bits(options_bits as u64);

    if jv.is_any_string() {
        let ptr = perry_runtime::js_get_string_pointer_unified(value) as *const StringHeader;
        if let Some(s) = string_from_header(ptr) {
            opts.replacement = s;
        }
        return opts;
    }

    if !jv.is_pointer() {
        return opts;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if obj.is_null() || perry_runtime::value::addr_class::is_handle_band(obj as usize) {
        return opts;
    }

    let field = |name: &[u8]| -> f64 {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
    };

    let replacement = field(b"replacement");
    if JSValue::from_bits(replacement.to_bits()).is_any_string() {
        let ptr = perry_runtime::js_get_string_pointer_unified(replacement) as *const StringHeader;
        if let Some(s) = string_from_header(ptr) {
            opts.replacement = s;
        }
    }
    opts.lower = perry_runtime::value::js_is_truthy(field(b"lower")) != 0;
    opts.strict = perry_runtime::value::js_is_truthy(field(b"strict")) != 0;
    // npm: `if (options.trim !== false) slug = slug.trim()` — absent
    // (undefined) means trim.
    let trim = field(b"trim");
    let trim_jv = JSValue::from_bits(trim.to_bits());
    opts.trim = trim_jv.is_undefined() || perry_runtime::value::js_is_truthy(trim) != 0;
    opts
}

/// Convert a string to a URL-friendly slug
/// slugify(string) -> string
#[no_mangle]
pub unsafe extern "C" fn js_slugify(input_ptr: *const StringHeader) -> *mut StringHeader {
    js_slugify_with_options(input_ptr, perry_runtime::JSValue::undefined().bits() as i64)
}

/// Convert a string to a URL-friendly slug with options.
/// `options_bits` carries the second JS argument as raw NaN-box bits:
/// `slugify(s, '_')` (string replacement) and
/// `slugify(s, { replacement, lower, strict, trim })` both route here.
#[no_mangle]
pub unsafe extern "C" fn js_slugify_with_options(
    input_ptr: *const StringHeader,
    options_bits: i64,
) -> *mut StringHeader {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let opts = options_from_bits(options_bits);
    let result = slugify_npm(&input, &opts);
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// Slugify with strict mode (only alphanumeric)
/// slugify(string, { strict: true }) -> string
#[no_mangle]
pub unsafe extern "C" fn js_slugify_strict(input_ptr: *const StringHeader) -> *mut StringHeader {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let opts = SlugifyOptions {
        strict: true,
        ..SlugifyOptions::default()
    };
    let result = slugify_npm(&input, &opts);
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
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
            replacement: "_".into(),
            ..SlugifyOptions::default()
        };
        assert_eq!(slugify_npm("foo bar baz", &opts), "foo_bar_baz");
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
        // npm defaults the replacement to '-' BEFORE the per-char
        // compare, so a literal '-' merges with adjacent whitespace.
        let opts = SlugifyOptions::default();
        assert_eq!(slugify_npm("a - b", &opts), "a-b");
        assert_eq!(slugify_npm("a-b", &opts), "a-b");
        assert_eq!(slugify_npm("foo_bar-baz", &opts), "foo_bar-baz");
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
