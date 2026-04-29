//! JSX runtime stubs (`js_jsx` / `js_jsxs`).
//!
//! These are link-clean no-op stubs that let Perry compile and link TSX/JSX
//! files without a JSX runtime package.  They accept the standard JSX
//! transform's `(type, props)` arguments (both NaN-boxed as `f64`) and return
//! `TAG_UNDEFINED`.
//!
//! A real JSX implementation (e.g. React, Preact, Solid) should be loaded via
//! `perry.compilePackages` / `perry/jsruntime`; when a real `jsx` function is
//! imported and in scope the HIR lowering resolves it to the imported symbol
//! instead of the bare ExternFuncRef.  These stubs only fire when no runtime
//! is wired — they make the linker happy and give a defined (rather than
//! crashing-on-null-dereference) result.
//!
//! # ABI note
//! The codegen in `lower_call.rs` routes `ExternFuncRef { name: "jsx" }` and
//! `"jsxs"` through a dedicated arm that passes ALL arguments as `double`
//! (NaN-boxed), bypassing the string→PTR conversion that the generic
//! ExternFuncRef path would apply to string literals.  Both stubs therefore
//! take `(f64, f64) -> f64`.  When more args are added in future (e.g. the
//! optional `key` parameter from the React 17+ transform) the arm and the
//! stubs should be updated together.

use crate::value::TAG_UNDEFINED;

/// No-op stub for the single-child JSX transform call `jsx(type, props)`.
///
/// Returns `TAG_UNDEFINED` as a NaN-boxed `f64`.
#[no_mangle]
pub extern "C" fn js_jsx(_type_arg: f64, _props: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// No-op stub for the multi-child JSX transform call `jsxs(type, props)`.
///
/// Returns `TAG_UNDEFINED` as a NaN-boxed `f64`.
#[no_mangle]
pub extern "C" fn js_jsxs(_type_arg: f64, _props: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
