//! A call whose callee is not a member expression must not hand the call's
//! "immediate callee member" flag (`lowering_call_callee`) to whatever member
//! is lowered first inside that callee. Before the fix, in
//! `pick(Number.isInteger)(w)` the argument `Number.isInteger` was lowered as
//! if it were the direct callee, and the member-tail reroute-undo
//! (#4596/#4627) collapsed it to `PropertyGet { GlobalGet(0), "isInteger" }`:
//! the program read `globalThis.isInteger` (undefined) instead of the
//! function. ECMA-262 13.3.6.1: a callee that is not a property reference is
//! evaluated as an ordinary expression; its sub-expressions are values.
//!
//! Each shape must keep the value form
//! `PropertyGet { PropertyGet { GlobalGet(0), "Number" }, "isInteger" }`.
//! The control: direct member calls (`console.log(..)`, `Number.isInteger(w)`)
//! still take their direct-callee forms. On the unfixed lowering the call,
//! nested-call, spread, `new`, tagged-template and `await` shapes fail; the
//! conditional/sequence (cleared in `lower_cond`/`lower_seq`), optional-call
//! and member-of-literal shapes passed there and pin that behavior.

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

const COLLAPSED: &str = "object: GlobalGet(0), property: \"isInteger\"";
const NAMESPACE: &str = "object: GlobalGet(0), property: \"Number\"";

fn lowered(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "callee_flag_leak.ts", &mut cache)
        .expect("parse");
    let module = lower_module(&parsed.module, "test", "callee_flag_leak.ts").expect("lower");
    format!("{module:?}")
}

fn assert_value_form(src: &str) {
    let hir = lowered(src);
    assert!(
        !hir.contains(COLLAPSED),
        "Number.isInteger collapsed to globalThis.isInteger in `{src}`:\n{hir}"
    );
    assert!(
        hir.contains(NAMESPACE),
        "Number.isInteger lost its namespace read in `{src}`:\n{hir}"
    );
}

const PRELUDE: &str = "function pick(f: any): any { return f }\nconst w: any = 3;\n";

#[test]
fn control_direct_member_calls_keep_the_direct_callee_forms() {
    // The flag still reaches a member callee: `console.log` takes the
    // reroute-undo form, `Number.isInteger(w)` the intrinsic.
    let hir = lowered(&format!("{PRELUDE}console.log(Number.isInteger(w));"));
    assert!(hir.contains("object: GlobalGet(0), property: \"log\""), "{hir}");
    assert!(hir.contains("NumberIsInteger(LocalGet("), "{hir}");
    assert!(!hir.contains(NAMESPACE), "{hir}");
}

#[test]
fn call_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}console.log(pick(Number.isInteger)(w));"));
}

#[test]
fn nested_call_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}console.log(pick(pick)(Number.isInteger)(w));"));
}

#[test]
fn spread_argument_of_call_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}console.log(pick(...[Number.isInteger])(w));"));
}

#[test]
fn optional_call_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}const p: any = pick;\nconsole.log(p?.(Number.isInteger)(w));"));
}

#[test]
fn new_in_callee_position() {
    assert_value_form(&format!(
        "{PRELUDE}function F(this: any, f: any): any {{ return f }}\nconsole.log(new (F as any)(Number.isInteger)(w));"
    ));
}

#[test]
fn tagged_template_in_callee_position() {
    assert_value_form(&format!(
        "{PRELUDE}function tag(s: any, f: any): any {{ return f }}\nconsole.log(tag`${{Number.isInteger}}`(w));"
    ));
}

#[test]
fn await_in_callee_position() {
    assert_value_form(&format!(
        "{PRELUDE}async function g(): Promise<any> {{ return (await Promise.resolve(Number.isInteger))(w) }}\ng();"
    ));
}

#[test]
fn conditional_and_sequence_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}console.log((w ? Number.isInteger : pick)(w));"));
    assert_value_form(&format!("{PRELUDE}console.log((0, Number.isInteger)(w));"));
}

#[test]
fn member_of_literal_in_callee_position() {
    assert_value_form(&format!("{PRELUDE}console.log([Number.isInteger][0](w));"));
    assert_value_form(&format!("{PRELUDE}console.log(({{ f: Number.isInteger }}).f(w));"));
}
