//! JSX runtime adapter (`js_jsx` / `js_jsxs`) with server-side HTML rendering.
//!
//! The codegen lowers every JSX element to a `(type, props)` call routed
//! here (see `crates/perry-codegen/src/lower_call/extern_func.rs`). Dispatch:
//!
//! - **User function components**: `type` is a callable closure — call it
//!   with `props`. `<App {...} />` returns whatever `App` returns (typically
//!   another JSX node).
//! - **Intrinsic elements** (`<div>`, `<h1>`, …): `type` is a tag string.
//!   We render the element to an HTML string *eagerly* — children are
//!   already-evaluated JSX nodes (inner `js_jsx` ran first), so we can splice
//!   their stored HTML — and box that HTML in a small object tagged with
//!   `JSX_NODE_CLASS_ID`. The object makes `typeof node === "object"` true,
//!   and `js_jsvalue_to_string` (to_string.rs) renders it back to its HTML.
//!   This is what makes `c.html(<Layout/>)` and `String(<div/>)` work under
//!   native compile (#1653).
//! - **Fragment marker** (`"__Fragment"`): renders the children only, with no
//!   wrapping tag.
//! - **`perry/tui` intrinsics** (`<Box>`/`<Text>`): rewritten at compile time
//!   before reaching here (#679); anything else unrecognised → `undefined`.
//!
//! # ABI note
//! `lower_call.rs` passes both args as `double` (NaN-boxed), so both adapters
//! are `(f64, f64) -> f64`.

use crate::closure::{js_closure_call1, ClosureHeader, CLOSURE_MAGIC};
use crate::object::ObjectHeader;
use crate::value::{JSValue, TAG_UNDEFINED};

/// Reserved class id marking a boxed server-rendered JSX node. Mirrors the
/// reserved-id convention in `object/instanceof.rs` (Date 0xFFFF0020,
/// ReadableStream 0xFFFF0060, …). Field 0 holds the rendered HTML string.
pub(crate) const JSX_NODE_CLASS_ID: u32 = 0xFFFF_00A0;

/// JSX call adapter for the single-child shape: `jsx(type, props)`.
#[no_mangle]
pub extern "C" fn js_jsx(type_arg: f64, props: f64) -> f64 {
    dispatch(type_arg, props)
}

/// JSX call adapter for the multi-child shape: `jsxs(type, props)`. Same
/// dispatch — the only difference is the SWC transform passes `children` as an
/// array, which `render_children` already flattens.
#[no_mangle]
pub extern "C" fn js_jsxs(type_arg: f64, props: f64) -> f64 {
    dispatch(type_arg, props)
}

fn dispatch(type_arg: f64, props: f64) -> f64 {
    let jsval = JSValue::from_bits(type_arg.to_bits());

    // Function component: call it with props. Its return value is already a
    // JSX node (or any value the component produced).
    if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<ClosureHeader>();
        if !ptr.is_null() && is_valid_closure(ptr) {
            return js_closure_call1(ptr, props);
        }
    }

    // Intrinsic element / Fragment: `type` is a tag string.
    if jsval.is_string() || jsval.is_short_string() {
        let tag = jsvalue_to_owned_string(type_arg);
        if tag == "__Fragment" {
            // Fragment: children only, no wrapping element.
            let html = render_children(get_field_by_name(props, "children"));
            return make_jsx_node(&html);
        }
        let attrs = render_props_attrs(props);
        let html = if is_void_element(&tag) {
            format!("<{tag}{attrs}/>")
        } else {
            let children = render_children(get_field_by_name(props, "children"));
            format!("<{tag}{attrs}>{children}</{tag}>")
        };
        return make_jsx_node(&html);
    }

    // Unrecognised type (e.g. a `perry/tui` intrinsic that the compile-time
    // rewriter didn't handle) → undefined, matching the historical behaviour.
    f64::from_bits(TAG_UNDEFINED)
}

/// HTML void elements — self-closing, never have children/closing tags.
fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Escape text content (`&`, `<`, `>`).
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape an attribute value (adds `"` on top of text escaping).
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Convert any NaN-boxed value to an owned Rust string via the runtime's
/// canonical stringifier.
fn jsvalue_to_owned_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        String::new()
    } else {
        crate::string::string_as_str(ptr).to_string()
    }
}

/// Read an object field by name, returning `undefined` for a non-object
/// receiver or a missing field.
fn get_field_by_name(obj_value: f64, name: &str) -> f64 {
    let jsval = JSValue::from_bits(obj_value.to_bits());
    if !jsval.is_pointer() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x10000 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let v = crate::object::js_object_get_field_by_name(obj, key);
    f64::from_bits(v.bits())
}

/// If `value` is a boxed JSX node, return its rendered HTML.
fn jsx_node_html(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x10000 {
        return None;
    }
    if unsafe { (*obj).class_id } != JSX_NODE_CLASS_ID {
        return None;
    }
    let field0 = crate::object::js_object_get_field(obj, 0);
    Some(jsvalue_to_owned_string(f64::from_bits(field0.bits())))
}

/// Box a rendered HTML string as a JSX-node object so it `typeof`s as
/// `"object"` and round-trips through `js_jsvalue_to_string`.
fn make_jsx_node(html: &str) -> f64 {
    let obj = crate::object::js_object_alloc(JSX_NODE_CLASS_ID, 1);
    let s = crate::string::js_string_from_bytes(html.as_ptr(), html.len() as u32);
    crate::object::js_object_set_field(obj, 0, JSValue::string_ptr(s));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Render `children` (a single value or an array) to HTML, splicing nested
/// JSX nodes verbatim and escaping text/number leaves.
fn render_children(children: f64) -> String {
    let mut out = String::new();
    render_one(children, &mut out);
    out
}

fn render_one(value: f64, out: &mut String) {
    let jsval = JSValue::from_bits(value.to_bits());
    // null / undefined / booleans render to nothing (React/Hono semantics).
    if jsval.is_undefined() || jsval.is_null() || jsval.is_bool() {
        return;
    }
    // Nested JSX node → splice its HTML as-is (already escaped/rendered).
    if let Some(html) = jsx_node_html(value) {
        out.push_str(&html);
        return;
    }
    // Array of children → flatten.
    if jsval.is_pointer() {
        let ptr: *const u8 = jsval.as_pointer();
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            let is_array = unsafe {
                let gc = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
            };
            if is_array {
                let arr = ptr as *const crate::ArrayHeader;
                let len = crate::array::js_array_length(arr);
                for i in 0..len {
                    let el = crate::array::js_array_get(arr, i);
                    render_one(f64::from_bits(el.bits()), out);
                }
                return;
            }
        }
    }
    // Text / number leaf → escaped text.
    out.push_str(&escape_text(&jsvalue_to_owned_string(value)));
}

/// Serialize an element's props into an attribute string (leading space per
/// attribute). Skips `children` / `key` / `ref`; maps `className` → `class`;
/// drops `null` / `undefined` / `false`; renders `true` as a bare boolean
/// attribute.
fn render_props_attrs(props: f64) -> String {
    let jsval = JSValue::from_bits(props.to_bits());
    if !jsval.is_pointer() {
        return String::new();
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x10000 {
        return String::new();
    }
    let keys = crate::object::js_object_keys(obj);
    if keys.is_null() {
        return String::new();
    }
    let mut out = String::new();
    let len = crate::array::js_array_length(keys);
    for i in 0..len {
        let key_val = crate::array::js_array_get(keys, i);
        let key = jsvalue_to_owned_string(f64::from_bits(key_val.bits()));
        if key == "children" || key == "key" || key == "ref" {
            continue;
        }
        let val = get_field_by_name(props, &key);
        let vjs = JSValue::from_bits(val.to_bits());
        if vjs.is_undefined() || vjs.is_null() {
            continue;
        }
        let name = if key == "className" {
            "class"
        } else {
            key.as_str()
        };
        if vjs.is_bool() {
            if vjs.as_bool() {
                out.push(' ');
                out.push_str(name);
            }
            continue;
        }
        let value_str = jsvalue_to_owned_string(val);
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_attr(&value_str));
        out.push('"');
    }
    out
}

/// Validate that `ptr` points at a real `ClosureHeader` (CLOSURE_MAGIC at
/// offset 12) before invoking it as a function component.
fn is_valid_closure(ptr: *const ClosureHeader) -> bool {
    let addr = ptr as u64;
    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
        return false;
    }
    let tag = unsafe { std::ptr::read_volatile((ptr as *const u8).add(12) as *const u32) };
    tag == CLOSURE_MAGIC
}
