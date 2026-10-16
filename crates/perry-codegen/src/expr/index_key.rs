//! Array-key admission: native integer provenance first, one shared exact
//! numeric normalizer otherwise. Rejected keys keep their original JS value.
use super::FnCtx;
use crate::function::LlFunction;
use crate::types::{DOUBLE, I1, I32, I64};
use perry_hir::{BinaryOp, Expr};

const NORMALIZE: &str = "perry_array_index_normalize_i32";

/// The specialized String lowering returns a UTF-16 code unit or NaN.
/// Uninitialized hoisted aliases may also be undefined; the ordered check
/// declines that tag as well. Every writer of an alias must produce a code
/// unit/NaN or a bounded i32. Parameters and captured cells are excluded.
pub(super) fn is_char_code(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    fn walk(ctx: &FnCtx<'_>, expr: &Expr, seen: &mut std::collections::HashSet<u32>) -> bool {
        match expr {
            Expr::Call { callee, .. } => matches!(callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                if property == "charCodeAt"
                    && crate::type_analysis::is_string_expr(ctx, object)
                    && crate::type_analysis::string_value_is_runtime_guaranteed(ctx, object)
                    && !crate::lower_call::property_get::patched_proto::guards(ctx, object, property)),
            Expr::LocalGet(id)
                if !ctx.boxed_vars.contains(id)
                    && !ctx.module_globals.contains_key(id)
                    && !ctx.closure_captures.contains_key(id)
                    && seen.insert(*id) =>
            {
                let initializer = ctx
                    .int_range_aliases
                    .get(id)
                    .is_some_and(|e| walk(ctx, e, &mut seen.clone()));
                initializer
                    && ctx.index_alias_writers.get(id).is_some_and(|values| {
                        values.iter().all(|value| {
                            writer_range(value).is_some_and(|(lo, hi)| {
                                lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX)
                            }) || walk(ctx, value, &mut seen.clone())
                        })
                    })
            }
            _ => false,
        }
    }
    walk(ctx, expr, &mut std::collections::HashSet::new())
}

/// Closed bounds for additional writers. In particular qs combines surrogate
/// code units using masks, a constant shift, OR, and addition. Never assume
/// that an arbitrary Number assignment is integral or within fptosi's range.
fn writer_range(expr: &Expr) -> Option<(i64, i64)> {
    match expr {
        Expr::Integer(n) if i32::try_from(*n).is_ok() => return Some((*n, *n)),
        Expr::Number(n)
            if n.is_finite()
                && n.fract() == 0.0
                && *n >= i32::MIN as f64
                && *n <= i32::MAX as f64 =>
        {
            return Some((*n as i64, *n as i64));
        }
        _ => {}
    }
    let Expr::Binary { op, left, right } = expr else {
        return None;
    };
    if *op == BinaryOp::BitAnd {
        let mask = writer_range(left).or_else(|| writer_range(right));
        if let Some((lo, hi)) = mask {
            if lo == hi && lo >= 0 && hi <= i64::from(i32::MAX) {
                return Some((0, hi));
            }
        }
    }
    let (lo, hi) = writer_range(left)?;
    let (rlo, rhi) = writer_range(right)?;
    match op {
        BinaryOp::Add => {
            let lower = lo.checked_add(rlo)?;
            let upper = hi.checked_add(rhi)?;
            (lower >= i64::from(i32::MIN) && upper <= i64::from(i32::MAX)).then_some((lower, upper))
        }
        BinaryOp::Shl
            if lo >= 0 && hi <= i64::from(i32::MAX) && rlo == rhi && (0..32).contains(&rlo) =>
        {
            let lower = lo.checked_shl(rlo as u32)?;
            let upper = hi.checked_shl(rlo as u32)?;
            (upper <= i64::from(i32::MAX)).then_some((lower, upper))
        }
        BinaryOp::BitOr
            if lo >= 0 && rlo >= 0 && hi <= i64::from(i32::MAX) && rhi <= i64::from(i32::MAX) =>
        {
            let max = hi.max(rhi) as u64;
            let bound = if max == 0 {
                0
            } else {
                (1u64 << (64 - max.leading_zeros())) - 1
            };
            Some((0, bound as i64))
        }
        _ => None,
    }
}

/// Preserve a widened native index even when lowering a later operand opened
/// another block. The cast's input dominates exactly the same uses as its
/// output; signed negatives and unsigned values above i32::MAX decline.
pub(super) fn normalize(ctx: &mut FnCtx<'_>, key: &str, expr: Option<&Expr>) -> String {
    let source = ctx
        .func
        .blocks()
        .iter()
        .rev()
        .find_map(|b| b.widened_i32_source(key));
    if let Some(index) = source {
        let valid = ctx.block().icmp_sge(I32, &index, "0");
        return ctx.block().select(I1, &valid, I32, &index, "-1");
    }
    if expr.is_some_and(|e| is_char_code(ctx, e)) {
        // The ordered comparison rejects the sole non-integer result, NaN.
        // Sanitize BEFORE fptosi: even a rejected conversion must not poison.
        let valid = ctx.block().fcmp("ord", key, key);
        let safe = ctx.block().select(I1, &valid, DOUBLE, key, "0.0");
        let index = ctx.block().fptosi(DOUBLE, &safe, I32);
        return ctx.block().select(I1, &valid, I32, &index, "-1");
    }
    publish(ctx);
    // This helper consumes only bits and cannot collect, throw, mutate the
    // receiver, or read mutable memory. Bypass general-call revalidation and
    // tell LLVM the same fact, so GET/SET can reuse admission for one SSA key.
    // Backend NaN-box operand loads come from the runtime's read-only table.
    let block = ctx.block();
    let index = block.next_reg();
    block.emit_raw(format!(
        "{index} = call i32 @{NORMALIZE}(double {key}) nounwind willreturn memory(none) \"gc-leaf-function\""
    ));
    index
}

fn publish(ctx: &mut FnCtx<'_>) {
    if ctx.pending_helpers.iter().any(|f| f.name == NORMALIZE) {
        return;
    }
    let mut f = LlFunction::new(NORMALIZE, I32, vec![(DOUBLE, "%key".into())]);
    f.set_gc_effects(ctx.func.gc_effects());
    f.linkage = "linkonce_odr".into();
    f.no_inline = true;
    let b = f.create_block("entry");
    let bits = b.bitcast_double_to_i64("%key");
    let tag = b.lshr(I64, &bits, "48");
    let boxed = b.icmp_eq(I64, &tag, crate::nanbox::INT32_TAG_TOP16_I64);
    b.cond_br(&boxed, "boxed.1", "raw.2");

    let b = f.create_block("boxed");
    let payload = b.trunc(I64, &bits, I32);
    let nonnegative = b.icmp_sge(I32, &payload, "0");
    let index = b.select(I1, &nonnegative, I32, &payload, "-1");
    b.ret(I32, &index);

    let b = f.create_block("raw");
    let ge = b.fcmp("oge", "%key", "0.0");
    let le = b.fcmp("ole", "%key", "2147483647.0");
    let range = b.and(I1, &ge, &le);
    b.cond_br(&range, "convert.3", "reject.4");

    // The ordered range branch dominates conversion. No speculative fptosi
    // can see NaN, a tag, an infinity, or an out-of-range Number.
    let b = f.create_block("convert");
    let raw = b.fptosi(DOUBLE, "%key", I32);
    let roundtrip = b.sitofp(I32, &raw, DOUBLE);
    let integral = b.fcmp("oeq", &roundtrip, "%key");
    let index = b.select(I1, &integral, I32, &raw, "-1");
    b.ret(I32, &index);

    f.create_block("reject").ret(I32, "-1");
    ctx.pending_helpers.push(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile_module, CompileOptions};
    use perry_hir::types::Type;
    use perry_hir::{Function, Module, Param, Stmt};

    fn read_ir(index: Expr, prelude: Vec<Stmt>) -> String {
        let mut module = Module::new("intkey.ts");
        let mut body = prelude;
        body.push(Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(index),
        })));
        module.functions.push(Function {
            id: 1,
            name: "read".into(),
            params: vec![
                Param {
                    id: 1,
                    name: "array".into(),
                    ty: Type::Array(Box::new(Type::Any)),
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                },
                Param {
                    id: 2,
                    name: "key".into(),
                    ty: Type::Number,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                },
                Param {
                    id: 3,
                    name: "text".into(),
                    ty: Type::String,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                },
            ],
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            type_params: vec![],
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        String::from_utf8(
            compile_module(
                &module,
                CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn dynamic_key_uses_one_linker_coalesced_leaf_normalizer() {
        let ir = read_ir(Expr::LocalGet(2), vec![]);
        assert!(
            ir.contains("define linkonce_odr i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(
            ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(
            ir.contains("aidx.runtime_key") && ir.contains("js_array_get_index_or_string"),
            "{ir}"
        );
    }

    #[test]
    fn bitwise_index_keeps_native_integer_and_exact_negative_fallback() {
        let ir = read_ir(
            Expr::Binary {
                op: perry_hir::BinaryOp::BitOr,
                left: Box::new(Expr::NumberCoerce(Box::new(Expr::LocalGet(2)))),
                right: Box::new(Expr::Integer(0)),
            },
            vec![],
        );
        assert!(
            !ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(
            ir.contains("aidx.runtime_key"),
            "negative keys need the exact path: {ir}"
        );
    }

    #[test]
    fn char_code_alias_admits_integer_but_preserves_nan() {
        let code = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::String("abc".into())),
                property: "charCodeAt".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(2)],
            type_args: vec![],
            byte_offset: 0,
        };
        let ir = read_ir(
            Expr::LocalGet(4),
            vec![Stmt::Let {
                id: 4,
                name: "c".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(code),
            }],
        );
        assert!(
            !ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(
            ir.contains("fcmp ord double") && ir.contains("aidx.runtime_key"),
            "{ir}"
        );
    }
    #[test]
    fn reassigned_char_code_is_a_negative_control_for_provenance() {
        let code = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::String("abc".into())),
                property: "charCodeAt".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(2)],
            type_args: vec![],
            byte_offset: 0,
        };
        let ir = read_ir(
            Expr::LocalGet(4),
            vec![
                Stmt::Let {
                    id: 4,
                    name: "c".into(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(code),
                },
                Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Number(1.5)))),
            ],
        );
        assert!(
            ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
    }
    #[test]
    fn bounded_surrogate_writer_keeps_char_code_admission() {
        let bin = |op, left, right| Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        };
        let surrogate = bin(
            BinaryOp::Add,
            Expr::Integer(0x10000),
            bin(
                BinaryOp::BitOr,
                bin(
                    BinaryOp::Shl,
                    bin(BinaryOp::BitAnd, Expr::LocalGet(4), Expr::Integer(0x3ff)),
                    Expr::Integer(10),
                ),
                bin(BinaryOp::BitAnd, Expr::LocalGet(2), Expr::Integer(0x3ff)),
            ),
        );
        let code = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::String("abc".into())),
                property: "charCodeAt".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(2)],
            type_args: vec![],
            byte_offset: 0,
        };
        let ir = read_ir(
            Expr::LocalGet(4),
            vec![
                Stmt::Let {
                    id: 4,
                    name: "c".into(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(code),
                },
                Stmt::Expr(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(4)),
                }),
                Stmt::Expr(Expr::LocalSet(4, Box::new(surrogate))),
            ],
        );
        let admission = ir.split("aidx.canonical").next().unwrap();
        assert!(
            !admission.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(admission.contains("fcmp ord double"), "{ir}");
    }

    #[test]
    fn parameter_redeclaration_cannot_supply_a_char_code_proof() {
        let code = Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::String("abc".into())),
                property: "charCodeAt".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::Integer(0)],
            type_args: vec![],
            byte_offset: 0,
        };
        let ir = read_ir(
            Expr::LocalGet(2),
            vec![Stmt::Let {
                id: 2,
                name: "key".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(code),
            }],
        );
        assert!(
            ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
    }
}
