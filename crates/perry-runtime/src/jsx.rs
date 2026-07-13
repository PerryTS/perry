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

use crate::closure::{js_closure_call1, ClosureHeader};
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

    // #6320: `<P />` where `P` is a `Proxy(<component fn>)`. A proxy value is a
    // small registry id NaN-boxed under POINTER_TAG, not a `ClosureHeader*`;
    // `is_valid_closure` used to probe `*(id + 12)` for CLOSURE_MAGIC behind an
    // 0x1000 floor and SIGSEGV'd on the unmapped low address. React calls the
    // component through the proxy's [[Call]] (apply trap, else forwarded to the
    // target), so do the same — components take no `this`, which is exactly what
    // the value-call path binds.
    if crate::proxy::js_proxy_is_proxy(type_arg) == 1 {
        if crate::proxy::proxy_wraps_callable(type_arg) {
            return unsafe { crate::closure::js_native_call_value(type_arg, &props, 1) };
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

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
        // `dangerouslySetInnerHTML={{ __html }}` (React/hono semantics): the
        // element's inner content is the raw, *unescaped* `__html` string, and
        // the prop is never serialized as an attribute (handled in
        // `render_props_attrs`). Its presence forces a normal (non-void)
        // element so the raw HTML has somewhere to live — e.g. `<div .../>`
        // with `__html` renders as `<div>...</div>`.
        let raw_inner_html = dangerous_inner_html(props);
        let html = if raw_inner_html.is_none() && is_void_element(&tag) {
            format!("<{tag}{attrs}/>")
        } else {
            let children = match raw_inner_html {
                Some(raw) => raw,
                None => render_children(get_field_by_name(props, "children")),
            };
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

/// React/hono `dangerouslySetInnerHTML={{ __html }}` support. If the props
/// object carries a `dangerouslySetInnerHTML` prop whose value is an object
/// with an `__html` field, return that field's raw string (to be spliced as
/// the element's inner content *unescaped*). Returns `None` when the prop is
/// absent or malformed (a non-object value, or no `__html`), in which case the
/// element renders normally.
fn dangerous_inner_html(props: f64) -> Option<String> {
    let prop = get_field_by_name(props, "dangerouslySetInnerHTML");
    let pjs = JSValue::from_bits(prop.to_bits());
    if pjs.is_undefined() || pjs.is_null() || !pjs.is_pointer() {
        return None;
    }
    let html_val = get_field_by_name(prop, "__html");
    let hjs = JSValue::from_bits(html_val.to_bits());
    if hjs.is_undefined() || hjs.is_null() {
        return None;
    }
    Some(jsvalue_to_owned_string(html_val))
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
        // `dangerouslySetInnerHTML` is consumed as raw inner content by the
        // caller (`dangerous_inner_html`), never serialized as an attribute —
        // otherwise it stringifies to `[object Object]` (#4827).
        if key == "dangerouslySetInnerHTML" {
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
///
/// #6320: this used to hand-roll the probe behind an `0x1000` floor, which is
/// an order of magnitude below `HANDLE_BAND_MAX` — every registry handle (proxy
/// id, fetch/zlib stream, stdlib id) sailed through and the `*(addr + 12)` read
/// faulted. `closure::is_closure_ptr` is the single owner of this check: it
/// rejects the handle band, the non-heap addresses, and misaligned pointers
/// before touching memory.
fn is_valid_closure(ptr: *const ClosureHeader) -> bool {
    crate::closure::is_closure_ptr(ptr as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Box a Rust string as a NaN-boxed JSValue string.
    fn str_val(s: &str) -> f64 {
        let p = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(p).bits())
    }

    /// Build a plain object (class id 0) with the given named string fields.
    fn obj_with(fields: &[(&str, f64)]) -> f64 {
        let obj = crate::object::js_object_alloc(0, 0);
        for (k, v) in fields {
            let key = crate::string::js_string_from_bytes(k.as_ptr(), k.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key, *v);
        }
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    }

    /// Render a JSX node value back to its HTML string.
    fn node_html(node: f64) -> String {
        jsx_node_html(node).expect("expected a boxed JSX node")
    }

    /// #4827: `dangerouslySetInnerHTML={{ __html }}` injects the raw,
    /// unescaped HTML as the element's children, never as an attribute. A
    /// self-closing source tag becomes a normal open/close pair.
    #[test]
    fn dangerously_set_inner_html_renders_raw_children() {
        let inner = obj_with(&[("__html", str_val("<b>hi</b>"))]);
        let props = obj_with(&[("dangerouslySetInnerHTML", inner)]);
        let node = js_jsx(str_val("div"), props);
        assert_eq!(node_html(node), "<div><b>hi</b></div>");
    }

    /// Real-world `<style dangerouslySetInnerHTML={{ __html: css }} />` case.
    #[test]
    fn dangerously_set_inner_html_style_keeps_css() {
        let inner = obj_with(&[("__html", str_val("body { color: red; }"))]);
        let props = obj_with(&[("dangerouslySetInnerHTML", inner)]);
        let node = js_jsx(str_val("style"), props);
        assert_eq!(node_html(node), "<style>body { color: red; }</style>");
    }

    /// The special prop must not leak into attribute serialization.
    #[test]
    fn dangerously_set_inner_html_not_emitted_as_attribute() {
        let inner = obj_with(&[("__html", str_val("x"))]);
        let props = obj_with(&[("dangerouslySetInnerHTML", inner)]);
        let node = js_jsx(str_val("div"), props);
        assert!(!node_html(node).contains("dangerouslySetInnerHTML"));
        assert!(!node_html(node).contains("[object Object]"));
    }

    /// Ordinary elements (and attribute/text escaping) are unaffected.
    #[test]
    fn ordinary_element_unaffected() {
        let props = obj_with(&[("className", str_val("x")), ("children", str_val("a<b"))]);
        let node = js_jsx(str_val("div"), props);
        assert_eq!(node_html(node), "<div class=\"x\">a&lt;b</div>");
    }

    /// A void element with no `dangerouslySetInnerHTML` stays self-closing.
    #[test]
    fn void_element_stays_self_closing() {
        let node = js_jsx(str_val("br"), f64::from_bits(TAG_UNDEFINED));
        assert_eq!(node_html(node), "<br/>");
    }

    /// A function component: `(props) => <span>{props.children}</span>`.
    extern "C" fn span_component(_closure: *const ClosureHeader, props: f64) -> f64 {
        js_jsx(str_val("span"), props)
    }

    /// Allocate `span_component` as a real capture-free closure value.
    fn span_component_value() -> f64 {
        let c = crate::closure::js_closure_alloc(span_component as *const u8, 0);
        f64::from_bits(JSValue::pointer(c as *const u8).bits())
    }

    /// Control: a plain closure component still renders.
    #[test]
    fn closure_function_component_renders() {
        let props = obj_with(&[("children", str_val("hi"))]);
        let node = js_jsx(span_component_value(), props);
        assert_eq!(node_html(node), "<span>hi</span>");
    }

    /// #6320: `<P />` where `P = new Proxy(Component, {})`. The proxy value is a
    /// small registry id, not a `ClosureHeader*`; the old `is_valid_closure`
    /// probe read `*(id + 12)` behind an `0x1000` floor and SIGSEGV'd. It must
    /// dispatch through the proxy's [[Call]] (here: no trap → forward to the
    /// target) and render the component.
    #[test]
    fn proxy_wrapped_function_component_dispatches_through_call() {
        let proxy = crate::proxy::js_proxy_new(span_component_value(), obj_with(&[]));
        let props = obj_with(&[("children", str_val("hi"))]);
        let node = js_jsx(proxy, props);
        assert_eq!(node_html(node), "<span>hi</span>");
    }

    /// A proxy over a NON-callable target is not a component — it must return
    /// undefined rather than fault or render garbage.
    #[test]
    fn proxy_over_non_callable_target_is_not_a_component() {
        let proxy = crate::proxy::js_proxy_new(obj_with(&[]), obj_with(&[]));
        let node = js_jsx(proxy, obj_with(&[]));
        assert_eq!(node.to_bits(), TAG_UNDEFINED);
    }

    /// The validator itself must reject every small-handle id (proxy ids, fetch
    /// / zlib streams, stdlib registry handles) WITHOUT dereferencing them.
    #[test]
    fn is_valid_closure_rejects_the_handle_band() {
        for handle in [
            0x1usize, 0x1000, 0x10000, 0x40000, 0xE0000, 0xF000D, 0xF_FFF8,
        ] {
            assert!(
                !is_valid_closure(handle as *const ClosureHeader),
                "handle {handle:#x} must not be probed as a ClosureHeader"
            );
        }
    }
}
