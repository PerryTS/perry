//! A string method's argument coercion runs while the receiver is held
//! (`lower_string_method/coerce.rs`).
//!
//! `("receiver-" + n).charAt(v)` with `v: any`: `ToNumber(v)` can run a user
//! `valueOf`, which can collect and evacuate the fresh receiver. The lowering
//! must (1) keep the receiver rooted across the coercion — so the coercion's
//! safepoint has a non-empty live set, where the unfixed lowering left it
//! EMPTY because only the arguments' own lowering fed the rooting decision —
//! and (2) run the coercion before it unboxes the receiver, so no raw
//! `StringHeader` address is held across it (the unfixed `charAt`, `indexOf`,
//! `slice`, … arms unboxed first).
//!
//! One case per coercing arm and coerced position. The receiver is a fresh
//! concatenation (its only reference is the register), and nothing else in the
//! function is used after the coercion, so a non-empty live set there is the
//! receiver's root.
//!
//! **Sabotage** — run against the pre-fix lowering (origin/main 6084f16e1):
//! every case fails, on the empty live set, and the `charAt`/`indexOf`/
//! `slice`/… cases also on the unbox-before-coercion order.

use super::*;
use perry_hir::BinaryOp;

const RECV_PARAM: u32 = 100;
const ARG_PARAM: u32 = 101;

fn method_module(name: &str, property: &str, args: Vec<Expr>) -> Module {
    let mut module = bare_module(name);
    let receiver = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("receiver-".to_string())),
        right: Box::new(Expr::LocalGet(RECV_PARAM)),
    };
    let param = |id: u32, name: &str, ty: Type| Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    module.functions = vec![Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(RECV_PARAM, "n", Type::Number),
            param(ARG_PARAM, "v", Type::Any),
        ],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(receiver),
                property: property.to_string(),
                byte_offset: 0,
            }),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    module
}

fn v() -> Expr {
    Expr::LocalGet(ARG_PARAM)
}

fn s(text: &str) -> Expr {
    Expr::String(text.to_string())
}

/// `(property, args, the coercion's runtime callee, the arm's consumer)`.
fn cases() -> Vec<(&'static str, Vec<Expr>, &'static str, &'static str)> {
    vec![
        (
            "charAt",
            vec![v()],
            "js_string_index_to_i32",
            "js_string_char_at",
        ),
        ("at", vec![v()], "js_string_index_to_i32", "js_string_at"),
        (
            "codePointAt",
            vec![v()],
            "js_string_index_to_i32",
            "js_string_code_point_at",
        ),
        (
            "charCodeAt",
            vec![v()],
            "js_string_number_coerce",
            "js_string_char_code_at",
        ),
        (
            "slice",
            vec![v()],
            "js_string_index_to_i32",
            "js_string_slice",
        ),
        (
            "substring",
            vec![Expr::Integer(1), v()],
            "js_string_end_index_to_i32",
            "js_string_substring",
        ),
        (
            "substr",
            vec![v()],
            "js_string_number_coerce",
            "js_string_substr",
        ),
        (
            "repeat",
            vec![v()],
            "js_string_number_coerce",
            "js_string_repeat",
        ),
        (
            "padStart",
            vec![v(), s("0")],
            "js_string_number_coerce",
            "js_string_pad_start",
        ),
        (
            "padEnd",
            vec![Expr::Integer(40), v()],
            "js_string_pad_fill",
            "js_string_pad_end",
        ),
        (
            "indexOf",
            vec![v()],
            "js_string_coerce",
            "js_string_index_of",
        ),
        (
            "lastIndexOf",
            vec![s("-"), v()],
            "js_string_number_coerce",
            "js_string_last_index_of_from",
        ),
        (
            "includes",
            vec![v()],
            "js_string_search_value_to_string",
            "js_string_index_of_from",
        ),
        (
            "startsWith",
            vec![s("r"), v()],
            "js_string_index_to_i32",
            "js_string_starts_with_at",
        ),
        (
            "endsWith",
            vec![v()],
            "js_string_search_value_to_string",
            "js_string_ends_with",
        ),
        ("anchor", vec![v()], "js_string_coerce", "js_string_anchor"),
        (
            "localeCompare",
            vec![v()],
            "js_string_coerce",
            "js_string_locale_compare",
        ),
        (
            "localeCompare",
            vec![s("a"), v()],
            "js_string_resolve_collator_args",
            "js_string_locale_compare_resolved",
        ),
    ]
}

#[test]
fn a_string_method_receiver_is_rooted_across_its_argument_coercion() {
    for target in NATIVE_TARGETS {
        let mut inline_arms = 0;
        for (i, (property, args, coercion, consumer)) in cases().into_iter().enumerate() {
            let argc = args.len();
            let name = format!("smc_{i}_{property}.ts");
            let module = method_module(&name, property, args);
            let ir = native_ir(&module, target, false);
            let symbol = probe_body_symbol(&ir, &name);
            let body = function_slice(&ir, &symbol);
            // #11915: a method with a checked entry is ONE call that takes the
            // receiver and the arguments boxed; no coercion and no unbox run in
            // the emitted code at all. The entry coerces an object argument
            // only on its rooted by-name path (`string_native_method` roots the
            // receiver and the arguments before any `valueOf`), so the window
            // this test pins does not exist at the site.
            if let Some(entry) = crate::builtin_proto::string_entry(property, argc) {
                assert!(
                    body.contains(&format!("@{entry}(")),
                    "[{target}] {property}: the call must go through {entry}:\n{body}"
                );
                for raw in [coercion, consumer, "js_get_string_pointer_unified"] {
                    assert!(
                        !body.contains(&format!("@{raw}(")),
                        "[{target}] {property}: {raw} belongs inside {entry}:\n{body}"
                    );
                }
                continue;
            }
            inline_arms += 1;
            assert!(
                body.contains(&format!("@{consumer}(")),
                "[{target}] {property}: the fixture must reach the string-method \
                 arm (no call to {consumer}):\n{body}"
            );

            // (1) the receiver is rooted across the coercion.
            let points = statepoints_of(&ir, target, &symbol);
            for point in points.at(coercion) {
                assert!(
                    !point.live.is_empty(),
                    "[{target}] {property}: nothing is live across `{coercion}` — \
                     the fresh receiver is held unrooted while the argument's \
                     user valueOf/toString can collect: {point:?}"
                );
            }

            // (2) no raw receiver address is held across the coercion: the
            // coercion precedes the receiver's unbox, wherever the arm unboxes
            // through the shared helper.
            let coerce_at = body
                .find(&format!("@{coercion}("))
                .unwrap_or_else(|| panic!("[{target}] {property}: no {coercion} call"));
            // A string-only search argument may be unboxed before a
            // numeric position is coerced. Check the handle the consumer
            // actually receives, rather than the first unbox in the function.
            let call_prefix = format!("@{consumer}(i64 ");
            let receiver_handle = body
                .lines()
                .find_map(|line| {
                    line.split_once(&call_prefix)
                        .map(|(_, rest)| rest.split([',', ')']).next().unwrap().trim())
                })
                .expect("consumer must take a receiver handle");
            let definition = format!("{receiver_handle} =");
            let receiver_def = body
                .lines()
                .find(|line| line.trim_start().starts_with(&definition))
                .expect("fresh receiver handle must have an SSA definition");
            let receiver_at = body.find(receiver_def).unwrap();
            assert!(
                coerce_at < receiver_at,
                "[{target}] {property}: the receiver handle consumed by                  {consumer} is derived BEFORE {coercion}:\n{body}"
            );
        }
        assert!(
            inline_arms >= 2,
            "[{target}] the arms without an entry (anchor, the 2-argument \
             localeCompare) must still be checked inline"
        );
    }
}

/// The control: number / string arguments need no coercion, so none is emitted
/// and the receiver is not rooted for it — `b.toString(16).padStart(2, "0")`
/// keeps its unrooted, call-free shape.
#[test]
fn a_string_method_with_primitive_arguments_emits_no_coercion() {
    for target in NATIVE_TARGETS {
        for (property, args) in [
            ("padStart", vec![Expr::Integer(2), s("0")]),
            ("charAt", vec![Expr::Integer(1)]),
            ("charAt", vec![Expr::Undefined]),
            ("includes", vec![s("x"), Expr::Integer(1)]),
            ("slice", vec![Expr::Undefined, Expr::Undefined]),
            ("indexOf", vec![s("x"), Expr::Integer(1)]),
            ("slice", vec![Expr::Integer(1), Expr::Integer(3)]),
        ] {
            let name = format!("smc_ctl_{property}.ts");
            let module = method_module(&name, property, args);
            let ir = native_ir(&module, target, false);
            let symbol = probe_body_symbol(&ir, &name);
            let body = function_slice(&ir, &symbol);
            for coercion in [
                "js_number_coerce",
                "js_string_number_coerce",
                "js_string_coerce",
                "js_string_pad_fill",
                "js_string_search_value_to_string",
                "js_string_search_string_to_string",
            ] {
                assert!(
                    !body.contains(&format!("@{coercion}(")),
                    "[{target}] {property} with primitive arguments must not emit \
                     {coercion}:\n{body}"
                );
            }
        }
    }
}

/// These locals carry initializer/representation evidence, with erased Any
/// annotations. Both f64 and integer storage, and string-operation results,
/// must avoid a second coercion.
#[test]
fn proven_string_argument_representations_are_call_free() {
    for target in NATIVE_TARGETS {
        for (property, init) in [
            ("charAt", Expr::Number(1.25)),
            ("charAt", Expr::Integer(2)),
            ("includes", s("b")),
            (
                "includes",
                Expr::Call {
                    byte_end: 0,
                    callee: Box::new(Expr::PropertyGet {
                        byte_end: 0,
                        object: Box::new(s("abc")),
                        property: "slice".to_string(),
                        byte_offset: 0,
                    }),
                    args: vec![Expr::Integer(1)],
                    type_args: vec![],
                    byte_offset: 0,
                },
            ),
            (
                "charAt",
                Expr::Call {
                    byte_end: 0,
                    callee: Box::new(Expr::PropertyGet {
                        byte_end: 0,
                        object: Box::new(s("abc")),
                        property: "indexOf".to_string(),
                        byte_offset: 0,
                    }),
                    args: vec![s("b")],
                    type_args: vec![],
                    byte_offset: 0,
                },
            ),
        ] {
            let name = format!("smc_repr_{property}.ts");
            let is_result = matches!(init, Expr::Call { .. });
            let mut module = method_module(
                &name,
                property,
                vec![if is_result {
                    init.clone()
                } else {
                    Expr::LocalGet(102)
                }],
            );
            module.functions[0].body.insert(
                0,
                Stmt::Let {
                    id: 102,
                    name: "known".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(init),
                },
            );
            let ir = native_ir(&module, target, false);
            let symbol = probe_body_symbol(&ir, &name);
            let body = function_slice(&ir, &symbol);
            for coercion in [
                "js_number_coerce",
                "js_string_number_coerce",
                "js_string_search_value_to_string",
                "js_string_search_string_to_string",
            ] {
                assert!(
                    !body.contains(&format!("@{coercion}(")),
                    "[{target}] {property}: redundant coercion {coercion}:\n{body}"
                );
            }
            assert!(
                !body.contains("strm.tonum"),
                "known representation has a guard: {body}"
            );
        }
    }
}

/// The inline decoder already admits and converts numeric indices. Forcing
/// the dispatch arm (extra arguments have no checked entry) must not eagerly
/// call the converter before that decoder; its call belongs to the slow arm.
#[test]
fn char_code_at_keeps_index_conversion_in_its_slow_arm() {
    for target in NATIVE_TARGETS {
        let name = "smc_char_code_conversion.ts";
        let module = method_module(
            name,
            "charCodeAt",
            vec![Expr::Number(1.25), Expr::Undefined],
        );
        let ir = native_ir(&module, target, false);
        let symbol = probe_body_symbol(&ir, name);
        let body = function_slice(&ir, &symbol);
        let slow = body.find("\ncca.slow.").expect("inline decoder slow arm");
        assert!(
            !body[..slow].contains("@js_string_number_index_to_i32("),
            "[{target}] eager conversion obstructs the inline decoder:\n{body}"
        );
        assert!(body[slow..].contains("@js_string_number_index_to_i32("));
    }
}
