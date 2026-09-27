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
    // #10943 CHANGED THIS CLAIM, deliberately. What stood here was
    // `!ir.contains(DISPATCH)` — "a proven receiver must not pay for method
    // dispatch" — and that premise is the bug: proving the receiver's KIND
    // proves nothing about an own property, so `d.getTime = () => "own"` ran
    // `Date.prototype.getTime` and returned a real timestamp. The dispatcher
    // now appears as the OTHER SIDE of one own-override diamond.
    //
    // What replaces it is the cost claim that is still worth pinning: a proven
    // receiver pays ONE predicate test and a branch, not the full tower —
    // exactly one dispatch call, reached only when the predicate says the
    // receiver may own the name.
    assert_eq!(
        ir.matches("js_receiver_may_own_named_method").count(),
        1,
        "a proven receiver pays exactly one own-override test:\n{ir}"
    );
    assert_eq!(
        ir.matches(DISPATCH).count(),
        1,
        "and reaches the dispatcher only as that test's other arm:\n{ir}"
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

// ---------------------------------------------------------------------------
// #11493: a receiver that PASSES the kind check can still own the method.
// ---------------------------------------------------------------------------

const OWN_TEST: &str = "call i32 @js_receiver_may_own_named_method(";
const OWN_FLAG: &str = "@PERRY_OWN_NAMED_PROP_INSTALLED";
const MAKE: &str = "perry_fn_kind_guard_ts__make";

/// Every register a call to `callee` assigns, in program order.
fn call_results(ir: &str, callee: &str) -> Vec<String> {
    let marker = format!(" = call double @{callee}(");
    ir.lines()
        .filter(|line| line.contains(&marker))
        .filter_map(|line| line.trim_start().split(" = ").next())
        .map(str::to_string)
        .collect()
}

#[test]
fn unproven_date_receiver_asks_the_own_override_test_before_the_builtin() {
    // `const d: any = new Date(0); d.getTime = () => 42; d.getTime()` passed
    // the Date check and returned 0: nothing between that check and the
    // builtin asked whether `d` owns `getTime`.
    let ir = main_ir(
        "kind_guard_own_get_time.ts",
        vec![method_call(any_value(), "getTime", Vec::new())],
    );
    assert!(
        ir.contains(OWN_FLAG) && ir.contains(OWN_TEST),
        "a Date receiver must take the own-override test before the builtin:\n{ir}"
    );
    assert_eq!(
        ir.matches(DISPATCH).count(),
        1,
        "an own getTime and a non-Date receiver share the one dispatch arm:\n{ir}"
    );
    assert_eq!(
        ir.matches(GET_TIME).count(),
        1,
        "the kind check still reads the time value the builtin arm returns:\n{ir}"
    );
    // The predicate may allocate. The receiver has to be in a rooted slot
    // across it, and both the predicate and the dispatcher read it back.
    let recv = call_results(&ir, MAKE).remove(0);
    crate::testing::temp_slots::assert_rooted_across(
        &ir,
        &recv,
        "js_receiver_may_own_named_method",
        "the own-override test",
    );
    crate::testing::temp_slots::assert_rooted_across(
        &ir,
        &recv,
        "js_typed_feedback_native_call_method_by_id",
        "the dispatch arm below the own-override test",
    );
}

#[test]
fn unproven_date_setter_roots_its_argument_across_the_own_override_test() {
    // A lone argument has no collecting operand after it, so before #11493 it
    // stayed an SSA register. The own-override test is a call that may
    // collect, so it is rooted now, and the builtin reads it from the root.
    let ir = main_ir(
        "kind_guard_own_set_utc_hours.ts",
        vec![method_call(any_value(), "setUTCHours", vec![any_value()])],
    );
    let results = call_results(&ir, MAKE);
    assert_eq!(results.len(), 2, "receiver and argument:\n{ir}");
    assert!(
        crate::testing::temp_slots::temp_root_slot_holding(&ir, &results[1]).is_some(),
        "the argument must be rooted across the own-override test:\n{ir}"
    );
    assert!(
        ir.contains(OWN_TEST) && ir.contains("call double @js_date_apply_setter("),
        "the setter stays behind the Date check and the own-override test:\n{ir}"
    );
    crate::testing::temp_slots::assert_rooted_across(
        &ir,
        &results[0],
        "js_date_apply_setter",
        "the setter below the own-override test",
    );
}

#[test]
fn unproven_array_receiver_asks_the_own_override_test_from_its_header() {
    let ir = main_ir(
        "kind_guard_own_to_sorted.ts",
        vec![method_call(any_value(), "toSorted", vec![Expr::Undefined])],
    );
    assert!(
        ir.contains(OWN_TEST) && ir.contains("load i16"),
        "a plain array tests its own named-property header bits before the \
         builtin, and asks the predicate only when they are set:\n{ir}"
    );
    assert!(
        !ir.contains(OWN_FLAG),
        "an array answers from its own header, not the global install flag:\n{ir}"
    );
}

#[test]
fn unproven_to_locale_string_asks_the_own_override_test_for_a_date_only() {
    let ir = main_ir(
        "kind_guard_own_to_locale_string.ts",
        vec![method_call(any_value(), "toLocaleString", Vec::new())],
    );
    assert!(
        ir.contains(OWN_TEST),
        "a Date receiver of toLocaleString() may own it:\n{ir}"
    );
    assert_eq!(
        ir.matches(OWN_TEST).count(),
        1,
        "only the Date arm asks; primitives and Symbols own nothing:\n{ir}"
    );
}

#[test]
fn unproven_number_method_skips_the_own_override_test() {
    // A number is a primitive: nothing can shadow its builtin on the receiver,
    // so its guard stays the inline tag test and nothing is rooted for it.
    let ir = main_ir(
        "kind_guard_own_to_fixed.ts",
        vec![method_call(any_value(), "toFixed", vec![Expr::Number(2.0)])],
    );
    assert!(
        !ir.contains(OWN_FLAG) && !ir.contains(OWN_TEST),
        "a Number receiver needs no own-override test:\n{ir}"
    );
}

#[test]
fn proven_date_names_outside_the_old_list_are_guarded() {
    // A proven Date reaches the chain's direct builtin for every name
    // `date_builtin` knows, so every one of them needs the #10943 diamond.
    // `setTime` and `getUTCHours` were missing from the hand-kept list.
    for name in ["setTime", "getUTCHours", "toUTCString", "toLocaleString"] {
        assert!(
            super::is_direct_date_builtin_name(name),
            "{name} lowers to a direct Date builtin"
        );
    }
    let ir = main_ir(
        "kind_guard_proven_set_time.ts",
        vec![
            method_call(
                Expr::DateNew(Vec::new()),
                "setTime",
                vec![Expr::Number(1.0)],
            ),
            method_call(Expr::DateNew(Vec::new()), "getUTCHours", Vec::new()),
        ],
    );
    assert_eq!(
        ir.matches(OWN_TEST).count(),
        2,
        "each proven Date call pays one own-override test:\n{ir}"
    );
}

#[test]
fn folded_date_formatters_are_guarded_and_a_numeric_to_locale_string_is_not() {
    let ir = main_ir(
        "kind_guard_folded_formatters.ts",
        vec![
            Stmt::Expr(Expr::DateToDateString(Box::new(Expr::DateNew(Vec::new())))),
            Stmt::Expr(Expr::DateGetTimezoneOffset(Box::new(Expr::DateNew(
                Vec::new(),
            )))),
        ],
    );
    assert_eq!(
        ir.matches(OWN_TEST).count(),
        2,
        "each folded Date formatter pays one own-override test:\n{ir}"
    );
    // `(12345).toLocaleString()` shares the Date node; a number owns nothing.
    let ir = main_ir(
        "kind_guard_folded_number_locale.ts",
        vec![Stmt::Expr(Expr::DateToLocaleString(Box::new(
            Expr::Number(12345.0),
        )))],
    );
    assert!(
        !ir.contains(OWN_FLAG) && !ir.contains(OWN_TEST),
        "a numeric toLocaleString keeps the plain fold:\n{ir}"
    );
}
