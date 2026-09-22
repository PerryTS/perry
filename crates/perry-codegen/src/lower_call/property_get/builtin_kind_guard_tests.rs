//! #10476: IR census for builtin-named Date / Number / Array method calls.
//!
//! The defect was a lowering choice, so the assertions are on the emitted
//! call, not on a predicate: a `getTime` / `setUTCHours` / `toFixed` /
//! `toSorted` call on an unproven receiver must reach universal method dispatch
//! on the non-builtin arm (where a user method lives), and a proven receiver
//! must keep the direct builtin call with no guard.

use perry_hir::types::Type;
use perry_hir::{Expr, Stmt};

use crate::compile_module;
use crate::temp_root_coverage::{entry_opts, module_with_init};

const DISPATCH: &str = "call double @js_typed_feedback_native_call_method_by_id(";
const GET_TIME: &str = "call double @js_date_get_time(";

/// The `main` body for `init`, with `make()` importable as an `any`-returning
/// function so its result is an unproven receiver.
fn main_ir(name: &str, init: Vec<Stmt>) -> String {
    let mut opts = entry_opts();
    opts.import_function_prefixes
        .insert("make".to_string(), "kind_guard_ts".to_string());
    let bytes = compile_module(&module_with_init(name, init), opts)
        .unwrap_or_else(|e| panic!("codegen failed for {name}: {e}"));
    let ir = String::from_utf8(bytes).expect("LLVM IR should be UTF-8");
    crate::testing::root_slots::function_slice(&ir, "main").to_string()
}

fn any_value() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "make".to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn method_call(object: Expr, property: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(object),
            property: property.to_string(),
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// The register a `= call double @callee(` line assigns.
fn call_result(ir: &str, callee: &str) -> String {
    let marker = format!(" = call double @{callee}(");
    let line = ir
        .lines()
        .find(|line| line.contains(&marker))
        .unwrap_or_else(|| panic!("no call to {callee}:\n{ir}"));
    line.trim_start()
        .split(" = ")
        .next()
        .expect("assignment")
        .to_string()
}

#[test]
fn unproven_date_getter_checks_the_receiver_kind() {
    let ir = main_ir(
        "kind_guard_get_time.ts",
        vec![method_call(any_value(), "getTime", Vec::new())],
    );
    assert_eq!(
        ir.matches(GET_TIME).count(),
        1,
        "one js_date_get_time call both classifies the receiver and is getTime's value:\n{ir}"
    );
    assert!(
        ir.contains("icmp ne i64"),
        "a Date is recognized by its time value differing from the receiver bits:\n{ir}"
    );
    assert!(
        ir.contains(DISPATCH),
        "a non-Date receiver must reach its own getTime through method dispatch:\n{ir}"
    );
    assert_eq!(
        ir.matches("= call double @perry_fn_kind_guard_ts__make(")
            .count(),
        1,
        "the receiver must be evaluated exactly once:\n{ir}"
    );
}

#[test]
fn unproven_date_getter_reuses_the_time_value_from_the_check() {
    let ir = main_ir(
        "kind_guard_utc_full_year.ts",
        vec![method_call(any_value(), "getUTCFullYear", Vec::new())],
    );
    let time = call_result(&ir, "js_date_get_time");
    assert!(
        ir.contains(&format!(
            "call double @js_date_get_utc_full_year(double {time})"
        )),
        "the getter must read the time value the check produced, not re-classify \
         the receiver:\n{ir}"
    );
    assert!(
        ir.contains(DISPATCH),
        "missing the method-dispatch arm:\n{ir}"
    );
}

#[test]
fn unproven_date_setter_forwards_every_argument_on_both_arms() {
    let ir = main_ir(
        "kind_guard_set_utc_hours.ts",
        vec![method_call(
            any_value(),
            "setUTCHours",
            vec![Expr::Number(1.0), Expr::Number(2.0)],
        )],
    );
    assert!(
        ir.contains(GET_TIME) && ir.contains("call double @js_date_apply_setter("),
        "a Date receiver must keep the setter behind a runtime Date check:\n{ir}"
    );
    assert!(
        ir.contains(DISPATCH),
        "missing the method-dispatch arm:\n{ir}"
    );
}

#[test]
fn unproven_to_locale_string_formats_primitives_dates_and_symbols_directly() {
    let ir = main_ir(
        "kind_guard_to_locale_string.ts",
        vec![method_call(any_value(), "toLocaleString", Vec::new())],
    );
    assert!(
        ir.contains("call double @js_value_to_locale_string(")
            && ir.contains(GET_TIME)
            && ir.contains("call i32 @js_is_symbol(")
            && ir.contains(DISPATCH),
        "primitives, Dates and Symbols format directly; heap objects dispatch:\n{ir}"
    );
}

#[test]
fn unproven_number_method_uses_an_inline_tag_check() {
    let ir = main_ir(
        "kind_guard_to_fixed.ts",
        vec![method_call(any_value(), "toFixed", vec![Expr::Number(2.0)])],
    );
    assert!(
        ir.contains("call i64 @js_number_to_fixed(") && ir.contains(DISPATCH),
        "toFixed on an unproven receiver needs both the Number and dispatch arms:\n{ir}"
    );
    assert!(
        !ir.contains(GET_TIME),
        "the Number check is an inline tag test, not a runtime call:\n{ir}"
    );
}

#[test]
fn unproven_to_sorted_checks_for_a_plain_array_header() {
    let ir = main_ir(
        "kind_guard_to_sorted.ts",
        vec![method_call(any_value(), "toSorted", vec![Expr::Undefined])],
    );
    assert!(
        ir.contains("call i64 @js_validate_array_comparator(")
            && ir.contains("call i64 @js_array_to_sorted_with_comparator(")
            && ir.contains(DISPATCH),
        "toSorted on an unproven receiver needs both the Array and dispatch arms:\n{ir}"
    );
    assert!(
        ir.contains("load i8"),
        "the Array check reads the GcHeader type inline:\n{ir}"
    );
}

#[test]
fn unproven_flatten_and_splice_methods_keep_the_dense_helpers_behind_the_guard() {
    let ir = main_ir(
        "kind_guard_flatten.ts",
        vec![
            method_call(any_value(), "flat", Vec::new()),
            method_call(any_value(), "flatMap", vec![Expr::Undefined]),
            method_call(any_value(), "toSpliced", vec![Expr::Number(1.0)]),
        ],
    );
    for helper in [
        "call i64 @js_array_flat(",
        "call i64 @js_array_flatMap(",
        "call i64 @js_array_to_spliced(",
    ] {
        assert!(
            ir.contains(helper),
            "missing the plain-array arm {helper}:\n{ir}"
        );
    }
    assert_eq!(
        ir.matches(DISPATCH).count(),
        3,
        "every call keeps a method-dispatch arm for a user method:\n{ir}"
    );
}

#[test]
fn unproven_reduce_right_with_three_arguments_is_plain_dispatch() {
    // Array.prototype.reduceRight's static lowering rejects three arguments,
    // but a user method may accept them: no guard, no compile error.
    let ir = main_ir(
        "kind_guard_reduce_right_arity.ts",
        vec![method_call(
            any_value(),
            "reduceRight",
            vec![Expr::Undefined, Expr::Undefined, Expr::Undefined],
        )],
    );
    assert!(
        ir.contains(DISPATCH) && !ir.contains("call double @js_array_reduce_right("),
        "an out-of-arity reduceRight must stay a method call:\n{ir}"
    );
}

#[test]
fn proven_receivers_keep_the_direct_builtin_call() {
    let ir = main_ir(
        "kind_guard_proven.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "n".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Number(1.25)),
            },
            method_call(Expr::LocalGet(1), "toFixed", vec![Expr::Number(1.0)]),
            method_call(Expr::DateNew(Vec::new()), "getTime", Vec::new()),
        ],
    );
    assert!(
        ir.contains("call i64 @js_number_to_fixed(")
            && ir.contains("call double @js_date_get_time("),
        "proven receivers must use the builtin directly:\n{ir}"
    );
    assert_eq!(
        ir.matches(GET_TIME).count(),
        1,
        "a proven Date calls the getter once, with no kind check:\n{ir}"
    );
    assert!(
        !ir.contains(DISPATCH),
        "a proven receiver must not pay for method dispatch:\n{ir}"
    );
}

#[test]
fn zero_argument_search_methods_compile_on_any_receiver_and_on_a_string() {
    let ir = main_ir(
        "kind_guard_zero_arg_search.ts",
        vec![
            method_call(any_value(), "endsWith", Vec::new()),
            method_call(any_value(), "includes", Vec::new()),
            method_call(
                Expr::String("xundefined".to_string()),
                "startsWith",
                Vec::new(),
            ),
        ],
    );
    assert!(
        ir.contains("call i32 @js_string_ends_with(")
            && ir.contains("call i32 @js_string_starts_with(")
            && ir.contains(DISPATCH),
        "an omitted searchString is `undefined`, not a compile error:\n{ir}"
    );
}
