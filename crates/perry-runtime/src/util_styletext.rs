//! `util.styleText(format, text[, options])` (#2514) — wrap `text` in the ANSI
//! escape sequences for one or more named formats from `util.inspect.colors`.
//!
//! - `format` is a color/modifier name or an array of them. Each must be a known
//!   format (or the special no-op `"none"`), else `ERR_INVALID_ARG_VALUE`.
//! - `text` must be a string, else `ERR_INVALID_ARG_TYPE` (no coercion).
//! - `options.validateStream` defaults to `true`: when the target stream (stdout)
//!   does not support color (e.g. piped, or `NO_COLOR`), the text is returned
//!   unstyled. `{ validateStream: false }` forces styling. Validation of
//!   `format`/`text` happens regardless of whether styling is applied.
//!
//! For an array of formats the opening codes are emitted in order and the
//! closing codes in reverse, matching Node:
//! `styleText(["red","bold"],"X")` → `\x1b[31m\x1b[1mX\x1b[22m\x1b[39m`.

use crate::url::{create_string_f64, get_string_content};
use crate::value::{js_nanbox_get_pointer, JSValue, TAG_FALSE};

/// `(name, open, close)` from Node's `util.inspect.colors`.
const COLORS: &[(&str, u16, u16)] = &[
    ("reset", 0, 0),
    ("bold", 1, 22),
    ("dim", 2, 22),
    ("italic", 3, 23),
    ("underline", 4, 24),
    ("blink", 5, 25),
    ("inverse", 7, 27),
    ("hidden", 8, 28),
    ("strikethrough", 9, 29),
    ("doubleunderline", 21, 24),
    ("black", 30, 39),
    ("red", 31, 39),
    ("green", 32, 39),
    ("yellow", 33, 39),
    ("blue", 34, 39),
    ("magenta", 35, 39),
    ("cyan", 36, 39),
    ("white", 37, 39),
    ("bgBlack", 40, 49),
    ("bgRed", 41, 49),
    ("bgGreen", 42, 49),
    ("bgYellow", 43, 49),
    ("bgBlue", 44, 49),
    ("bgMagenta", 45, 49),
    ("bgCyan", 46, 49),
    ("bgWhite", 47, 49),
    ("framed", 51, 54),
    ("overlined", 53, 55),
    ("gray", 90, 39),
    ("redBright", 91, 39),
    ("greenBright", 92, 39),
    ("yellowBright", 93, 39),
    ("blueBright", 94, 39),
    ("magentaBright", 95, 39),
    ("cyanBright", 96, 39),
    ("whiteBright", 97, 39),
    ("bgGray", 100, 49),
    ("bgRedBright", 101, 49),
    ("bgGreenBright", 102, 49),
    ("bgYellowBright", 103, 49),
    ("bgBlueBright", 104, 49),
    ("bgMagentaBright", 105, 49),
    ("bgCyanBright", 106, 49),
    ("bgWhiteBright", 107, 49),
];

fn color_codes(name: &str) -> Option<(u16, u16)> {
    COLORS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, o, c)| (*o, *c))
}

fn is_string_f64(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_any_string()
}

fn throw_invalid_format() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The argument 'format' must be a string or an array of strings naming \
         a recognized color or modifier.",
        "ERR_INVALID_ARG_VALUE",
    )
}

fn throw_invalid_text() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The \"text\" argument must be of type string.",
        "ERR_INVALID_ARG_TYPE",
    )
}

/// Collect the format names from a `string | string[]` value, throwing
/// `ERR_INVALID_ARG_VALUE` for anything else.
fn collect_format_names(format: f64) -> Vec<String> {
    if is_string_f64(format) {
        return vec![get_string_content(format)];
    }
    // Array of strings?
    if crate::array::js_array_is_array(format) != 0.0 {
        let arr_ptr = js_nanbox_get_pointer(format) as *const crate::array::ArrayHeader;
        let len = unsafe { crate::array::js_array_length(arr_ptr) };
        let mut names = Vec::with_capacity(len as usize);
        for i in 0..len {
            let el = unsafe { crate::array::js_array_get_f64(arr_ptr, i) };
            if !is_string_f64(el) {
                throw_invalid_format();
            }
            names.push(get_string_content(el));
        }
        return names;
    }
    throw_invalid_format();
}

/// Whether output should be styled: `validateStream:false` forces styling;
/// otherwise only when stdout supports color (a TTY and `NO_COLOR` unset).
fn should_style(options: f64) -> bool {
    let opt_v = JSValue::from_bits(options.to_bits());
    if opt_v.is_pointer() {
        let key = b"validateStream";
        let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let obj_ptr = js_nanbox_get_pointer(options) as *const crate::object::ObjectHeader;
        let field = crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr);
        if field.to_bits() == TAG_FALSE {
            return true; // validateStream:false → always style
        }
    }
    // Default: style only if stdout (fd 1) supports color.
    std::env::var_os("NO_COLOR").is_none() && crate::tty::is_tty_fd(1)
}

#[no_mangle]
pub extern "C" fn js_util_style_text(format: f64, text: f64, options: f64) -> f64 {
    // 1) Validate format (always, even when not styling).
    let names = collect_format_names(format);
    let mut pairs: Vec<(u16, u16)> = Vec::with_capacity(names.len());
    for name in &names {
        if name == "none" {
            continue; // documented no-op format
        }
        match color_codes(name) {
            Some(p) => pairs.push(p),
            None => throw_invalid_format(),
        }
    }

    // 2) Validate text (must be a string).
    if !is_string_f64(text) {
        throw_invalid_text();
    }
    let text_s = get_string_content(text);

    // 3) Decide styling.
    if pairs.is_empty() || !should_style(options) {
        return create_string_f64(&text_s);
    }

    // 4) Wrap: opens in order, closes in reverse.
    let mut out = String::with_capacity(text_s.len() + pairs.len() * 10);
    for (open, _) in &pairs {
        out.push_str(&format!("\u{1b}[{open}m"));
    }
    out.push_str(&text_s);
    for (_, close) in pairs.iter().rev() {
        out.push_str(&format!("\u{1b}[{close}m"));
    }
    create_string_f64(&out)
}
