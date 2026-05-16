//! JSX runtime adapter (`js_jsx` / `js_jsxs`).
//!
//! These intercept the SWC JSX-transform's `(type, props)` calls and
//! dispatch:
//!
//! - **User function components**: when `type` is a callable closure
//!   (NaN-boxed POINTER to a `ClosureHeader`), we call it with `props`
//!   as the single argument. This is what makes `<App />` work for
//!   user-defined `function App(props) { … }` components.
//! - **Fragment marker**: when `type` is the inline `"__Fragment"`
//!   string, we return `props.children` directly (the SWC transform
//!   already collected fragment children into the `children` field).
//! - **Everything else**: returns `TAG_UNDEFINED` — including JSX
//!   pointing at built-in intrinsics like `<Box>` / `<Text>` from
//!   `perry/tui`. Those are reachable through the JSX runtime only
//!   after the per-intrinsic compile-time rewriter lands (#679 Phase 2
//!   follow-up). Until then, use the function-call form `Box([...])` /
//!   `Text("…")` for `perry/tui` widgets.
//!
//! # ABI note
//! The codegen in `lower_call.rs` routes `ExternFuncRef { name: "jsx" }` and
//! `"jsxs"` through a dedicated arm that passes ALL arguments as `double`
//! (NaN-boxed), bypassing the string→PTR conversion that the generic
//! ExternFuncRef path would apply to string literals. Both adapters
//! therefore take `(f64, f64) -> f64`. When more args are added in
//! future (e.g. the optional `key` parameter from the React 17+
//! transform) the arm and the adapters should be updated together.

use crate::closure::{js_closure_call1, ClosureHeader, CLOSURE_MAGIC};
use crate::value::{JSValue, TAG_UNDEFINED};

/// JSX call adapter for the single-child shape: `jsx(type, props)`.
///
/// Dispatches based on `type_arg`'s NaN-boxing tag — see the module
/// docs for the dispatch matrix.
#[no_mangle]
pub extern "C" fn js_jsx(type_arg: f64, props: f64) -> f64 {
    dispatch(type_arg, props)
}

/// JSX call adapter for the multi-child shape: `jsxs(type, props)`.
/// Same dispatch as `js_jsx` — the only semantic difference between
/// `jsx` and `jsxs` is that the SWC transform tells the runtime
/// `children` is an array vs. a single node, and our dispatch handles
/// both uniformly.
#[no_mangle]
pub extern "C" fn js_jsxs(type_arg: f64, props: f64) -> f64 {
    dispatch(type_arg, props)
}

fn dispatch(type_arg: f64, props: f64) -> f64 {
    let jsval = JSValue::from_bits(type_arg.to_bits());

    // Pointer-tagged JSValue ⇒ candidate function component. Validate
    // that it looks like a real closure (CLOSURE_MAGIC at offset 12)
    // before calling — non-closure POINTERs (object handles, etc.)
    // shouldn't be invoked as functions.
    if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<ClosureHeader>();
        if !ptr.is_null() && is_valid_closure(ptr) {
            return js_closure_call1(ptr, props);
        }
    }

    // Falls through to undefined for unrecognised types — including
    // perry/tui intrinsics (`<Box>` / `<Text>`) and Fragment markers.
    // See module docs.
    f64::from_bits(TAG_UNDEFINED)
}

/// Validate that `ptr` points at a real `ClosureHeader`. Mirrors the
/// safety check `get_valid_func_ptr` does at the call site (reads the
/// `CLOSURE_MAGIC` tag at offset 12 to distinguish closures from raw
/// object pointers).
fn is_valid_closure(ptr: *const ClosureHeader) -> bool {
    let addr = ptr as u64;
    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
        return false;
    }
    let tag = unsafe { std::ptr::read_volatile((ptr as *const u8).add(12) as *const u32) };
    tag == CLOSURE_MAGIC
}
