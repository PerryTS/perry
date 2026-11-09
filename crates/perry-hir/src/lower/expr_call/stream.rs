//! Stream-API helpers used by `expr_call/mod.rs`.
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the only consumers are inside `lower_call_inner` in this module.

use swc_ecma_ast as ast;

use super::super::LoweringContext;

/// Issue #562 — does `method` name a stream-API method on the given
/// stream module? Used to gate the native-instance method rerouting so
/// user-declared subclass methods fall through to regular class method
/// dispatch. Mirrors the methods table in
/// `crates/perry-codegen/src/lower_call.rs`'s `module == "<stream>"`
/// arms — see also the parallel `is_stream_api_member` in
/// `expr_member.rs`.
pub(super) fn is_stream_api_method(module: &str, method: &str) -> bool {
    match module {
        "readable_stream" => matches!(
            method,
            "getReader" | "cancel" | "tee" | "pipeTo" | "pipeThrough" | "locked"
        ),
        "readable_stream_reader" => {
            matches!(method, "read" | "releaseLock" | "cancel" | "closed")
        }
        "writable_stream" => matches!(method, "getWriter" | "abort" | "close" | "locked"),
        "writable_stream_writer" => matches!(
            method,
            "write" | "close" | "abort" | "releaseLock" | "closed" | "ready" | "desiredSize"
        ),
        "transform_stream" => matches!(method, "readable" | "writable"),
        _ => false,
    }
}
