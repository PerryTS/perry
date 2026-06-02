//! #1671 — `hono/jsx/server` and `hono/jsx/streaming` import surfaces.
//!
//! Perry renders JSX with the built-in `js_jsx` runtime (codegen routes every
//! `jsx`/`jsxs` call there unconditionally; `.tsx` does not need a package
//! import for that built-in path). These two hono submodules exist for code
//! that imports the runtime / streaming helpers *directly* rather than relying
//! on the JSX transform:
//!
//!   - `hono/jsx/server`    → `jsx`, `jsxs`, `Fragment`, `JSXNode`
//!   - `hono/jsx/streaming` → `renderToReadableStream`, `Suspense`
//!
//! Without this module the imports resolved to the `TAG_TRUE` sentinel (so
//! `typeof jsx === "boolean"`). The thunks below forward to the real `js_jsx`
//! renderer so `jsx(...)`/`jsxs(...)`/`Fragment`/`Suspense` actually produce
//! HTML, and `renderToReadableStream` renders eagerly to a single-chunk
//! `ReadableStream` (via a stdlib-registered callback) so the result is a real
//! Web stream that `getReader()` / `for await` can drain.

use std::sync::atomic::{AtomicPtr, Ordering};

use crate::closure::ClosureHeader;
use crate::value::{JSValue, TAG_UNDEFINED};

#[inline]
fn fragment_marker_f64() -> f64 {
    let s = crate::string::js_string_from_bytes(b"__Fragment".as_ptr(), 10);
    f64::from_bits(JSValue::string_ptr(s).bits())
}

/// `jsx(type, props)` — forward to the built-in renderer.
pub(crate) extern "C" fn thunk_hono_jsx(
    _closure: *const ClosureHeader,
    type_arg: f64,
    props: f64,
) -> f64 {
    crate::jsx::js_jsx(type_arg, props)
}

/// `jsxs(type, props)` — multi-child shape, same dispatch.
pub(crate) extern "C" fn thunk_hono_jsxs(
    _closure: *const ClosureHeader,
    type_arg: f64,
    props: f64,
) -> f64 {
    crate::jsx::js_jsxs(type_arg, props)
}

/// `Fragment` — when used as a component (`jsx(Fragment, { children })`) it
/// renders its children with no wrapping element. Reuse `js_jsx`'s Fragment
/// path by passing the `"__Fragment"` marker tag.
pub(crate) extern "C" fn thunk_hono_fragment(_closure: *const ClosureHeader, props: f64) -> f64 {
    crate::jsx::js_jsx(fragment_marker_f64(), props)
}

/// `Suspense` — Perry renders synchronously (server-side), so a Suspense
/// boundary just renders its children (the fallback is never needed because
/// there is no streaming-suspension point). Same shape as Fragment.
pub(crate) extern "C" fn thunk_hono_suspense(_closure: *const ClosureHeader, props: f64) -> f64 {
    crate::jsx::js_jsx(fragment_marker_f64(), props)
}

/// `JSXNode` — hono's JSX node *class*. Perry never constructs it directly
/// (`js_jsx` boxes nodes itself), so this is an exposed stub returning the
/// props/value it was handed. Its sole purpose is to make the named export
/// resolve with `typeof === "function"`.
pub(crate) extern "C" fn thunk_hono_jsxnode(_closure: *const ClosureHeader, arg: f64) -> f64 {
    arg
}

/// stdlib callback: `(html_string_value) -> ReadableStream handle`. Registered
/// by perry-stdlib (`bundled-streams`) so the runtime can hand back a real Web
/// stream without depending on perry-stdlib. When unset (streams not linked)
/// `renderToReadableStream` falls back to returning the rendered HTML node.
static JSX_RENDER_STREAM_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

type RenderStreamFn = unsafe extern "C" fn(f64) -> f64;

/// Called from perry-stdlib at dispatch init to wire up stream creation.
#[no_mangle]
pub unsafe extern "C" fn js_register_jsx_render_stream(f: RenderStreamFn) {
    JSX_RENDER_STREAM_FN.store(f as *mut (), Ordering::Release);
}

/// `renderToReadableStream(node, options?)` — render `node` to HTML eagerly,
/// then emit it as a single-chunk `ReadableStream` (the WHATWG return type).
/// `node` is already a boxed JSX node (or any stringifiable value); the runtime
/// stringifier renders it to its HTML. Falls back to returning the node itself
/// when no stream backend is linked.
pub(crate) extern "C" fn thunk_hono_render_to_readable_stream(
    _closure: *const ClosureHeader,
    node: f64,
    _options: f64,
) -> f64 {
    // Render the node to its HTML string via the canonical stringifier
    // (handles JSX_NODE_CLASS_ID → its stored HTML, plain strings, etc.).
    let html_ptr = crate::value::js_jsvalue_to_string(node);
    let html_value = if html_ptr.is_null() {
        node
    } else {
        f64::from_bits(JSValue::string_ptr(html_ptr).bits())
    };
    let raw = JSX_RENDER_STREAM_FN.load(Ordering::Acquire);
    if raw.is_null() {
        // No stream backend linked — hand back the rendered HTML node so the
        // value is at least usable (degraded vs. a real ReadableStream).
        return html_value;
    }
    let f: RenderStreamFn = unsafe { std::mem::transmute(raw) };
    unsafe { f(html_value) }
}

/// `undefined` helper for any export we deliberately don't back.
#[allow(dead_code)]
pub(crate) extern "C" fn thunk_hono_undefined(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
