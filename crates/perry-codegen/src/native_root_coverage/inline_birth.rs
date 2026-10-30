//! Inline class births keep live operands in native relocation bundles.
use super::*;
use perry_hir::{Class, ClassField};

fn fixture(with_args: bool) -> Module {
    let mut module = bare_module("native_class_birth.ts");
    module.classes.push(Class {
        id: 1,
        name: "Pair".into(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            origin: perry_hir::ClassFieldOrigin::Definition,
            name: "v".into(),
            key_expr: None,
            ty: Type::Any,
            init: Some(Expr::Object(Vec::new())),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    });
    if with_args {
        for name in ["left", "right"] {
            module.classes[0].fields.push(ClassField {
                origin: perry_hir::ClassFieldOrigin::Definition,
                name: name.into(),
                key_expr: None,
                ty: Type::Any,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            });
        }
        module.classes[0].constructor = Some(Function {
            id: 21,
            name: "constructor".into(),
            type_params: Vec::new(),
            params: [10, 11]
                .into_iter()
                .map(|id| Param {
                    id,
                    name: format!("arg{id}"),
                    ty: Type::Any,
                    default: None,
                    decorators: Vec::new(),
                    is_rest: false,
                    arguments_object: None,
                })
                .collect(),
            return_type: Type::Void,
            body: ["left", "right"]
                .into_iter()
                .zip([10, 11])
                .map(|(property, id)| {
                    Stmt::Expr(Expr::PropertySet {
                        object: Box::new(Expr::This),
                        property: property.into(),
                        value: Box::new(Expr::LocalGet(id)),
                        byte_offset: 0,
                        byte_end: 0,
                    })
                })
                .collect(),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
    }
    module.init.push(Stmt::Expr(Expr::New {
        byte_end: 0,
        class_name: "Pair".into(),
        args: if with_args {
            vec![Expr::Object(Vec::new()), Expr::Object(Vec::new())]
        } else {
            Vec::new()
        },
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    module
}

// Read a live member's SSA provenance through boxing, phi selection and
// relocations. Stop at unrelated calls: their operands are different values.
pub(super) fn comes_from_call(body: &str, value: &str, callee: &str, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    let defs: std::collections::HashMap<_, _> = body
        .lines()
        .filter_map(|l| l.trim().split_once(" = "))
        .collect();
    let Some(rhs) = defs.get(value) else {
        return false;
    };
    if rhs.contains(&format!("@{callee}(")) {
        return true;
    }
    // Native root reloads launder bits through a tied, non-collecting asm
    // operand. Its result aliases that operand; it is not an unrelated call.
    if let Some(operand) = rhs.strip_prefix("call i64 asm \"\", \"=r,0\"(ptr addrspace(1) ") {
        let value = operand.split(')').next().unwrap();
        return comes_from_call(body, value, callee, depth - 1);
    }
    if rhs.contains("@llvm.experimental.gc.result.") {
        let token = rhs.split_once("(token ").unwrap().1.trim_end_matches(')');
        return defs
            .get(token)
            .is_some_and(|sp| callee_after_elementtype(sp).as_deref() == Some(callee));
    }
    if rhs.contains("@llvm.experimental.gc.relocate.") {
        let token = rhs
            .split_once("(token ")
            .unwrap()
            .1
            .split(',')
            .next()
            .unwrap();
        let index: usize = rhs
            .rsplit_once("i32 ")
            .unwrap()
            .1
            .split(')')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let point = parse_statepoint(defs[token]);
        return comes_from_call(body, &point.live[index], callee, depth - 1);
    }
    if ["inttoptr ", "ptrtoint ", "bitcast ", "or ", "and ", "phi "]
        .iter()
        .any(|op| rhs.starts_with(op))
    {
        return rhs
            .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .filter(|r| r.starts_with('%'))
            .any(|r| comes_from_call(body, r, callee, depth - 1));
    }
    false
}

// True when `value` is a native root re-read (the tied, non-collecting asm
// launder every root reload emits), followed through boxing and bit casts.
// A register that reaches a call without passing a re-read holds the value
// from before any later collection point.
fn comes_from_root_reread(body: &str, value: &str, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    let Some(rhs) = body
        .lines()
        .filter_map(|l| l.trim().split_once(" = "))
        .find(|(lhs, _)| *lhs == value)
        .map(|(_, rhs)| rhs)
    else {
        return false;
    };
    if rhs.starts_with("call i64 asm \"\", \"=r,0\"(ptr addrspace(1) ") {
        return true;
    }
    if ["inttoptr ", "ptrtoint ", "bitcast ", "or ", "and "]
        .iter()
        .any(|op| rhs.starts_with(op))
    {
        return rhs
            .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
            .filter(|r| r.starts_with('%'))
            .all(|r| comes_from_root_reread(body, r, depth - 1));
    }
    false
}

// #12462: a complete birth stores its arguments in the nursery arm. Argument 0
// was lowered before argument 1's allocation, which collects, so every
// dynamic field value the fast arm stores must be a re-read of its root, never
// the register the argument's own lowering produced.
fn assert_fast_fields_are_rereads(body: &str) {
    let fast = body
        .split("\nalloc.fast.")
        .nth(1)
        .expect("fast birth must be live");
    let fast = fast.split("\nalloc.slow.").next().unwrap();
    let operands: Vec<_> = fast
        .lines()
        .filter(|l| l.contains(" = insertelement "))
        .filter_map(|l| {
            let value = l.split(", i64 ").nth(1)?.split(',').next()?.trim();
            value.starts_with('%').then_some(value)
        })
        .collect();
    assert_eq!(operands.len(), 2, "both fields are dynamic: {fast}");
    for value in operands {
        assert!(
            comes_from_root_reread(body, value, 16),
            "fast birth stores {value}, not a root re-read:\n{body}"
        );
    }
}

fn assert_constructor_instance(body: &str) {
    let ctor_point = body
        .lines()
        .filter(|l| {
            l.contains("@llvm.experimental.gc.statepoint.")
                && callee_after_elementtype(l).as_deref() == Some("js_object_alloc_plain")
        })
        .last()
        .expect("allocating constructor body");
    let point = parse_statepoint(ctor_point);
    assert!(
        point
            .live
            .iter()
            .any(|value| comes_from_call(body, value, "perry_birth_class", 32)),
        "the constructor lost the birthed instance: {ctor_point}\n{body}"
    );
    let token = ctor_point.trim().split_once(" = ").unwrap().0;
    for (i, value) in point.live.iter().enumerate() {
        if comes_from_call(body, value, "perry_birth_class", 32) {
            assert!(
                body.lines()
                    .any(|l| l.contains("@llvm.experimental.gc.relocate.")
                        && l.contains(&format!("(token {token}, i32 {i}, i32 {i})"))),
                "constructor must relocate the birthed instance {value}"
            );
        }
    }
}

// Every live bundle member must have its own relocate tied to this token and
// its actual bundle index. A relocate for another call proves nothing.
fn assert_relocated(body: &str, callee: &str, minimum: usize) {
    let points: Vec<_> = body
        .lines()
        .filter(|l| {
            l.contains("@llvm.experimental.gc.statepoint.")
                && callee_after_elementtype(l).as_deref() == Some(callee)
        })
        .collect();
    assert_eq!(points.len(), 1, "one collecting {callee} call:\n{body}");
    let point = parse_statepoint(points[0]);
    assert!(
        point.live.len() >= minimum,
        "{callee} lost live values: {point:?}\n{body}"
    );
    let token = points[0].trim().split_once(" = ").unwrap().0;
    for i in 0..point.live.len() {
        assert!(
            body.lines()
                .any(|l| l.contains("@llvm.experimental.gc.relocate.")
                    && l.contains(&format!("(token {token}, i32 {i}, i32 {i})"))),
            "{callee} did not relocate live member {i}: {point:?}\n{body}"
        );
    }
}

#[test]
fn complete_birth_writes_final_fields_and_relocates_slow_arguments() {
    let mut module = fixture(true);
    module.classes[0].fields.remove(0); // remove the observable field initializer
    module.classes[0].name = "__AnonShape_native_birth".into();
    if let Stmt::Expr(Expr::New { class_name, .. }) = &mut module.init[0] {
        *class_name = module.classes[0].name.clone();
    }
    assert!(module.classes[0].is_literal_shape());
    for target in NATIVE_TARGETS {
        let ir = native_ir(&module, target, true);
        let body = function_slice(&ir, "main");
        assert_eq!(
            body.matches("call preserve_mostcc i64 @perry_birth_class(")
                .count(),
            1,
            "{body}"
        );
        let fast = body
            .split("\nalloc.fast.")
            .nth(1)
            .expect("fast birth must be live");
        let fast = fast.split("\nalloc.slow.").next().unwrap();
        // Root re-reads are tied asm operands, not calls: nothing in the fast
        // arm may collect.
        assert!(
            !fast
                .lines()
                .any(|l| l.contains("call ") && !l.contains("call i64 asm \"\", \"=r,0\"")),
            "{fast}"
        );
        assert_eq!(
            fast.matches("store <2 x i64>").count(),
            2,
            "header and final fields: {fast}"
        );
        assert_eq!(fast.matches("insertelement <2 x i64>").count(), 2, "{fast}");
        assert_fast_fields_are_rereads(body);
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&ir, target, "complete_birth").unwrap();
        let body = function_slice(&rewritten, "main");
        assert_relocated(body, "perry_birth_class", 2);
        assert!(
            body.contains("ctor_prologue.slow."),
            "slow constructor must stay live"
        );
    }
    // One field still reserves two slots: the spare Any slot must be seeded.
    module.classes[0].fields.pop();
    let ctor = module.classes[0].constructor.as_mut().unwrap();
    ctor.params.pop();
    ctor.body.pop();
    if let Stmt::Expr(Expr::New { args, .. }) = &mut module.init[0] {
        args.pop();
    }
    let ir = native_ir(&module, NATIVE_TARGETS[0], true);
    let body = function_slice(&ir, "main");
    assert!(
        body.contains(&format!(
            ", i64 {}, i32 1",
            crate::nanbox::TAG_UNDEFINED_I64
        )),
        "unused floor slot must be undefined: {body}"
    );
}

#[test]
fn class_birth_slow_arm_relocates_arguments_and_constructor_relocates_instance() {
    for target in NATIVE_TARGETS {
        let args_ir = native_ir(&fixture(true), target, true);
        let args_rewritten =
            crate::inprocess::statepoint_rewritten_ir(&args_ir, target, "birth_args").unwrap();
        assert_relocated(
            function_slice(&args_rewritten, "main"),
            "perry_birth_class",
            2,
        );
        let args_body = function_slice(&args_rewritten, "main");
        let birth = args_body
            .lines()
            .find(|l| {
                l.contains("@llvm.experimental.gc.statepoint.")
                    && callee_after_elementtype(l).as_deref() == Some("perry_birth_class")
            })
            .unwrap();
        let point = parse_statepoint(birth);
        assert_eq!(
            point
                .live
                .iter()
                .filter(|value| comes_from_call(args_body, value, "js_object_alloc_plain", 32))
                .count(),
            2,
            "both fresh argument values must be listed at the slow birth: {point:?}"
        );
        let ir = native_ir(&fixture(false), target, true);
        let body = function_slice(&ir, "main");
        let calls: Vec<_> = body
            .lines()
            .filter(|l| l.contains("call ") && l.contains("@perry_birth_class("))
            .collect();
        assert_eq!(
            calls.len(),
            2,
            "non-complete constructor keeps shared fast and slow births:\n{body}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|l| l.contains("\"gc-leaf-function\""))
                .count(),
            1
        );
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&ir, target, "birth_roots").unwrap();
        let body = function_slice(&rewritten, "main");
        // The no-argument inline fixture still has a collecting slow arm.
        assert_relocated(body, "perry_birth_class", 0);
        let birth = body
            .lines()
            .find(|l| {
                l.contains("@llvm.experimental.gc.statepoint.")
                    && callee_after_elementtype(l).as_deref() == Some("perry_birth_class")
            })
            .unwrap();
        assert!(
            birth.contains("ptr null,"),
            "only the slow arm may be a safepoint: {birth}"
        );
        assert!(
            body.lines().any(
                |l| l.contains("call preserve_mostcc i64 @perry_birth_class(")
                    && !l.contains("ptr null,")
            ),
            "the non-complete birth must retain its shared leaf initializer"
        );

        // The object allocation is the inline field initializer. Its
        // live set must carry the freshly birthed instance across the body.
        assert_constructor_instance(body);
        // Negative control: remove only the native stores of the newly born
        // instance. The birth call remains a statepoint in the control.
        let pre_body = function_slice(&ir, "main");
        let instance_phi = pre_body
            .lines()
            .find(|l| l.contains(" = phi i64 "))
            .expect("merged class birth")
            .trim()
            .split_once(" = ")
            .unwrap()
            .0;
        let homes: Vec<_> = crate::testing::temp_slots::slot_traffic(pre_body)
            .keys()
            .filter(|slot| {
                let isolated = pre_body
                    .lines()
                    .filter(|l| {
                        !l.trim().starts_with("store ")
                            || l.contains(&format!(", ptr {slot},"))
                            || l.trim().ends_with(&format!(", ptr {slot}"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                crate::testing::temp_slots::slot_holding(&isolated, instance_phi).is_some()
            })
            .cloned()
            .collect();
        assert!(
            homes.len() >= 2,
            "instance temp and inline this must both be rooted:\n{ir}"
        );
        let mut broken = ir.clone();
        for line in pre_body
            .lines()
            .filter(|l| l.trim().starts_with("store ptr addrspace(1) %"))
        {
            if homes.iter().any(|home| {
                line.contains(&format!(", ptr {home},"))
                    || line.trim().ends_with(&format!(", ptr {home}"))
            }) {
                let rest = line.trim().split_once(", ptr ").unwrap().1;
                broken =
                    broken.replace(line, &format!("  store ptr addrspace(1) null, ptr {rest}"));
            }
        }
        assert_ne!(broken, ir, "negative control must remove a live root store");
        let broken =
            crate::inprocess::statepoint_rewritten_ir(&broken, target, "birth_control").unwrap();
        let broken_body = function_slice(&broken, "main");
        assert!(
            std::panic::catch_unwind(|| assert_constructor_instance(broken_body)).is_err(),
            "constructor assertion accepted removed instance coverage"
        );
        assert_relocated(broken_body, "perry_birth_class", 0);
    }
}
