//! `util.styleText(format, text[, options])` ANSI formatting helper.
//!
//! Perry implements the Node-visible formatting contract for the built-in
//! style names and the non-interactive color gate. Passing
//! `{ validateStream: false }` forces ANSI output, which is the stable way
//! tests and libraries request coloring independent of stdout TTY state.

use crate::array::{js_array_get_f64, js_array_length, ArrayHeader};
use crate::object::{js_object_get_field_by_name_f64, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::{JSValue, TAG_FALSE, TAG_TRUE, TAG_UNDEFINED};

#[derive(Clone, Copy)]
pub(crate) struct AnsiStyle {
    pub name: &'static str,
    pub open: i32,
    pub close: i32,
}

pub(crate) const INSPECT_COLOR_STYLES: &[AnsiStyle] = &[
    AnsiStyle {
        name: "reset",
        open: 0,
        close: 0,
    },
    AnsiStyle {
        name: "bold",
        open: 1,
        close: 22,
    },
    AnsiStyle {
        name: "dim",
        open: 2,
        close: 22,
    },
    AnsiStyle {
        name: "italic",
        open: 3,
        close: 23,
    },
    AnsiStyle {
        name: "underline",
        open: 4,
        close: 24,
    },
    AnsiStyle {
        name: "blink",
        open: 5,
        close: 25,
    },
    AnsiStyle {
        name: "inverse",
        open: 7,
        close: 27,
    },
    AnsiStyle {
        name: "hidden",
        open: 8,
        close: 28,
    },
    AnsiStyle {
        name: "strikethrough",
        open: 9,
        close: 29,
    },
    AnsiStyle {
        name: "doubleunderline",
        open: 21,
        close: 24,
    },
    AnsiStyle {
        name: "black",
        open: 30,
        close: 39,
    },
    AnsiStyle {
        name: "red",
        open: 31,
        close: 39,
    },
    AnsiStyle {
        name: "green",
        open: 32,
        close: 39,
    },
    AnsiStyle {
        name: "yellow",
        open: 33,
        close: 39,
    },
    AnsiStyle {
        name: "blue",
        open: 34,
        close: 39,
    },
    AnsiStyle {
        name: "magenta",
        open: 35,
        close: 39,
    },
    AnsiStyle {
        name: "cyan",
        open: 36,
        close: 39,
    },
    AnsiStyle {
        name: "white",
        open: 37,
        close: 39,
    },
    AnsiStyle {
        name: "bgBlack",
        open: 40,
        close: 49,
    },
    AnsiStyle {
        name: "bgRed",
        open: 41,
        close: 49,
    },
    AnsiStyle {
        name: "bgGreen",
        open: 42,
        close: 49,
    },
    AnsiStyle {
        name: "bgYellow",
        open: 43,
        close: 49,
    },
    AnsiStyle {
        name: "bgBlue",
        open: 44,
        close: 49,
    },
    AnsiStyle {
        name: "bgMagenta",
        open: 45,
        close: 49,
    },
    AnsiStyle {
        name: "bgCyan",
        open: 46,
        close: 49,
    },
    AnsiStyle {
        name: "bgWhite",
        open: 47,
        close: 49,
    },
    AnsiStyle {
        name: "framed",
        open: 51,
        close: 54,
    },
    AnsiStyle {
        name: "overlined",
        open: 53,
        close: 55,
    },
    AnsiStyle {
        name: "gray",
        open: 90,
        close: 39,
    },
    AnsiStyle {
        name: "redBright",
        open: 91,
        close: 39,
    },
    AnsiStyle {
        name: "greenBright",
        open: 92,
        close: 39,
    },
    AnsiStyle {
        name: "yellowBright",
        open: 93,
        close: 39,
    },
    AnsiStyle {
        name: "blueBright",
        open: 94,
        close: 39,
    },
    AnsiStyle {
        name: "magentaBright",
        open: 95,
        close: 39,
    },
    AnsiStyle {
        name: "cyanBright",
        open: 96,
        close: 39,
    },
    AnsiStyle {
        name: "whiteBright",
        open: 97,
        close: 39,
    },
    AnsiStyle {
        name: "bgGray",
        open: 100,
        close: 49,
    },
    AnsiStyle {
        name: "bgRedBright",
        open: 101,
        close: 49,
    },
    AnsiStyle {
        name: "bgGreenBright",
        open: 102,
        close: 49,
    },
    AnsiStyle {
        name: "bgYellowBright",
        open: 103,
        close: 49,
    },
    AnsiStyle {
        name: "bgBlueBright",
        open: 104,
        close: 49,
    },
    AnsiStyle {
        name: "bgMagentaBright",
        open: 105,
        close: 49,
    },
    AnsiStyle {
        name: "bgCyanBright",
        open: 106,
        close: 49,
    },
    AnsiStyle {
        name: "bgWhiteBright",
        open: 107,
        close: 49,
    },
];

// #3870-adjacent (util/style-text): Node's `styleText` validates `format`
// against `Object.keys(util.inspect.colors)` — exactly the 44 names in
// `INSPECT_COLOR_STYLES`. It does NOT accept British/camelCase aliases
// (`grey`, `bgGrey`, `strikeThrough`, `swapColors`, `doubleUnderline`,
// `crossedout`, `conceal`, `faint`, `blackBright`, …); those throw
// `ERR_INVALID_ARG_VALUE`. The previous alias table made Perry too lenient.

const VALID_FORMATS_MESSAGE: &str = "'reset', 'bold', 'dim', 'italic', 'underline', 'blink', \
    'inverse', 'hidden', 'strikethrough', 'doubleunderline', 'black', 'red', 'green', \
    'yellow', 'blue', 'magenta', 'cyan', 'white', 'bgBlack', 'bgRed', 'bgGreen', \
    'bgYellow', 'bgBlue', 'bgMagenta', 'bgCyan', 'bgWhite', 'framed', 'overlined', \
    'gray', 'redBright', 'greenBright', 'yellowBright', 'blueBright', 'magentaBright', \
    'cyanBright', 'whiteBright', 'bgGray', 'bgRedBright', 'bgGreenBright', \
    'bgYellowBright', 'bgBlueBright', 'bgMagentaBright', 'bgCyanBright', \
    'bgWhiteBright'";

fn style_for(name: &str) -> Option<AnsiStyle> {
    INSPECT_COLOR_STYLES
        .iter()
        .find(|style| style.name == name)
        .copied()
}

fn js_string_content(value: f64) -> Option<String> {
    crate::builtins::jsvalue_string_content(value)
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn received_for_invalid_value(value: f64) -> String {
    if let Some(s) = js_string_content(value) {
        return format!("'{s}'");
    }
    let jsvalue = JSValue::from_bits(value.to_bits());
    if jsvalue.is_int32() {
        return jsvalue.as_int32().to_string();
    }
    if jsvalue.is_number() && value.is_finite() {
        return value.to_string();
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_format(value: f64) -> ! {
    let message = format!(
        "The argument 'format' must be one of: {VALID_FORMATS_MESSAGE}. Received {}",
        received_for_invalid_value(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn throw_invalid_text(value: f64) -> ! {
    let message = format!(
        "The \"text\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_options(value: f64) -> ! {
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_validate_stream(value: f64) -> ! {
    let message = format!(
        "The \"options.validateStream\" property must be of type boolean. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn heap_ptr_with_gc_type(value: f64, expected_type: u8) -> Option<*const u8> {
    let jsvalue = JSValue::from_bits(value.to_bits());
    if !jsvalue.is_pointer() {
        return None;
    }
    let ptr = jsvalue.as_pointer::<u8>();
    if ptr.is_null()
        || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000
        || crate::closure::is_closure_ptr(ptr as usize)
        || !crate::object::is_valid_obj_ptr(ptr)
    {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type == expected_type {
        Some(ptr)
    } else {
        None
    }
}

fn array_ptr(value: f64) -> Option<*const ArrayHeader> {
    heap_ptr_with_gc_type(value, crate::gc::GC_TYPE_ARRAY).map(|ptr| ptr as *const ArrayHeader)
}

fn object_ptr(value: f64) -> Option<*const ObjectHeader> {
    heap_ptr_with_gc_type(value, crate::gc::GC_TYPE_OBJECT).map(|ptr| ptr as *const ObjectHeader)
}

fn get_prop(obj: *const ObjectHeader, name: &[u8]) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name_f64(obj, key)
}

fn format_styles(format: f64) -> Vec<AnsiStyle> {
    if let Some(name) = js_string_content(format) {
        if name == "none" {
            return Vec::new();
        }
        return vec![style_for(&name).unwrap_or_else(|| throw_invalid_format(format))];
    }

    if let Some(arr) = array_ptr(format) {
        let mut styles = Vec::new();
        for i in 0..js_array_length(arr) {
            let item = js_array_get_f64(arr, i);
            let Some(name) = js_string_content(item) else {
                throw_invalid_format(item);
            };
            if name == "none" {
                continue;
            }
            styles.push(style_for(&name).unwrap_or_else(|| throw_invalid_format(item)));
        }
        return styles;
    }

    throw_invalid_format(format)
}

fn should_style(options: f64) -> bool {
    let jsvalue = JSValue::from_bits(options.to_bits());
    let validate_stream = if jsvalue.is_undefined() {
        true
    } else {
        let Some(obj) = object_ptr(options) else {
            throw_invalid_options(options);
        };
        let validate_stream = get_prop(obj, b"validateStream");
        match validate_stream.to_bits() {
            TAG_UNDEFINED => true,
            TAG_TRUE => true,
            TAG_FALSE => false,
            _ => throw_invalid_validate_stream(validate_stream),
        }
    };

    if !validate_stream {
        return true;
    }
    if let Some(force) = std::env::var_os("FORCE_COLOR") {
        let force = force.to_string_lossy();
        return !force.is_empty() && force != "0" && force.to_ascii_lowercase() != "false";
    }
    if std::env::var_os("NO_COLOR").is_some() || std::env::var_os("NODE_DISABLE_COLORS").is_some() {
        return false;
    }
    crate::tty::is_tty_fd(1)
}

fn csi(code: i32) -> String {
    format!("\x1b[{code}m")
}

fn text_with_inner_resets(text: &str, styles: &[AnsiStyle]) -> String {
    let mut out = text.to_string();
    let mut seen_closes: Vec<i32> = Vec::new();
    for style in styles {
        if seen_closes.contains(&style.close) {
            continue;
        }
        seen_closes.push(style.close);
        let close = csi(style.close);
        let replacement = if style.close == 22 {
            let mut reopened = close.clone();
            for reopen in styles
                .iter()
                .rev()
                .filter(|candidate| candidate.close == style.close)
            {
                reopened.push_str(&csi(reopen.open));
            }
            reopened
        } else {
            csi(style.open)
        };
        out = out.replace(&close, &replacement);
    }
    out
}

fn apply_styles(text: &str, styles: &[AnsiStyle]) -> String {
    if styles.is_empty() {
        return text.to_string();
    }

    let mut out = String::new();
    for style in styles {
        out.push_str(&csi(style.open));
    }
    out.push_str(&text_with_inner_resets(text, styles));
    for style in styles.iter().rev() {
        out.push_str(&csi(style.close));
    }
    out
}

#[no_mangle]
pub extern "C" fn js_util_style_text(format: f64, text: f64, options: f64) -> f64 {
    let styles = format_styles(format);
    let Some(text) = js_string_content(text) else {
        throw_invalid_text(text);
    };

    let out = if should_style(options) {
        apply_styles(&text, &styles)
    } else {
        text
    };
    string_value(&out)
}
