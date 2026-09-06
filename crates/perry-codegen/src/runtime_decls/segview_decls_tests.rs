//! #9843: the segment-view tier's runtime entry points must be DECLARED, not
//! only called.
//!
//! The tier's HIR-level tests cannot see this. They assert the rewrite emits
//! `Call(ExternFuncRef "js_segments_view_next", …)`, which it did — and the
//! build still failed, because the module carried the call and no `declare`:
//!
//! ```text
//! perry_llvm_….ll:5172:22: error: use of undefined value '@js_segments_view_next'
//!   %r75 = call double @js_segments_view_next(double %r74)
//! ```
//!
//! The in-process LLVM parse rejects the whole module, so this is not a
//! degraded build, it is no build at all. This test closes the gap between
//! "the lowering emits the call" and "the module can be parsed": remove any one
//! of the five registrations in `strings.rs` and it fails by name.

use super::declare_phase_b_strings;
use crate::module::LlModule;

/// Every entry point the segment-view lowering can emit, with the signature
/// taken from `perry-runtime/src/intl/segments_view.rs`. `regexp_test` is
/// `(cursor, regex)` — cursor first; it was relayed the other way round once
/// and the source settled it.
const SEGVIEW_DECLS: &[(&str, usize)] = &[
    ("js_segments_view_open", 2),
    ("js_segments_view_next", 1),
    ("js_segments_view_code_point_at", 2),
    ("js_segments_view_segment", 1),
    ("js_segments_view_regexp_test", 2),
];

#[test]
fn every_segment_view_entry_point_is_declared() {
    let mut m = LlModule::new("arm64-apple-macosx");
    declare_phase_b_strings(&mut m);
    let declared: Vec<(&str, &str)> = m.declaration_lines().collect();

    for (name, arity) in SEGVIEW_DECLS {
        let line = declared
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| {
                panic!(
                    "`{name}` is never declared, so any module that calls it fails the LLVM \
                     parse with \"use of undefined value\". Register it in \
                     `runtime_decls/strings.rs` beside `js_for_of_next`."
                )
            })
            .1;
        // Arity is checked because a wrong one is accepted by the parser and
        // then miscompiles the call: LLVM would coerce or drop an argument.
        let params = line
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map(|(inner, _)| {
                if inner.trim().is_empty() {
                    0
                } else {
                    inner.split(',').count()
                }
            })
            .unwrap_or_else(|| panic!("malformed declare line for `{name}`: {line}"));
        assert_eq!(
            params, *arity,
            "`{name}` is declared with {params} parameters, runtime defines {arity}: {line}"
        );
        assert!(
            line.contains("double"),
            "`{name}` must use the NaN-boxed f64 ABI like every other js_* entry: {line}"
        );
    }
}
