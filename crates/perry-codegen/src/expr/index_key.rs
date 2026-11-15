//! Array-key admission: native integer provenance first, one shared exact
//! numeric normalizer otherwise. Rejected keys keep their original JS value.
use super::FnCtx;
use crate::function::LlFunction;
use crate::types::{DOUBLE, I1, I32};
use perry_hir::Expr;

const NORMALIZE: &str = "perry_array_index_normalize_i32";

/// The builtin `String.prototype.charCodeAt` returns a UTF-16 code unit or
/// NaN, so this is a fact about values produced while `Pristine(String)`
/// holds: [`normalize`] admits through it only after testing that bit at the
/// read. The bit is sticky, so clear at the read means clear at every write.
/// Uninitialized hoisted aliases may also be undefined; the ordered check
/// declines that tag as well. Every writer of an alias must produce a code
/// unit/NaN or a bounded i32. Parameters and captured cells are excluded.
pub(super) fn is_char_code(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    fn walk(ctx: &FnCtx<'_>, expr: &Expr, seen: &mut std::collections::HashSet<u32>) -> bool {
        match expr {
            // Closed World 5d: every write of this body local is a code unit
            // of a proven String, NaN, or a bounded int32, and no read runs
            // before a write (`TypedLocals::code_units`). The proof is
            // whole-body, so it holds here whatever the lowering order saw.
            // `index_alias_writers` must list it as well: that keeps out
            // parameters (a sloppy `arguments` object aliases them), closure
            // references, and locals with writes neither walk can see.
            Expr::LocalGet(id)
                if ctx.code_unit_locals.contains(id)
                    && ctx.index_alias_writers.contains_key(id) =>
            {
                true
            }
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
                            crate::collectors::typed_locals::int32_writer_range(value).is_some_and(
                                |(lo, hi)| lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX),
                            ) || walk(ctx, value, &mut seen.clone())
                        })
                    })
            }
            _ => false,
        }
    }
    walk(ctx, expr, &mut std::collections::HashSet::new())
}

/// Preserve a widened native index even when lowering a later operand opened
/// another block. The cast's input dominates exactly the same uses as its
/// output; signed negatives and unsigned values above i32::MAX decline.
pub(super) fn normalize(ctx: &mut FnCtx<'_>, key: &str, expr: Option<&Expr>) -> String {
    normalize_keyed(ctx, key, expr, false)
}

/// [`normalize`] for a consumer whose every element arm is behind an
/// unsigned bound (`idx <u n`, `n < 2^31`) and whose every declined key
/// takes one arm that reads the original key (N10, [`unsigned_bound`]):
/// a widened native index is handed over as it is. A negative `i32` (or a
/// `uitofp` source at or above 2^31) is at least 2^31 unsigned, so the bound
/// declines it exactly as it declines `-1`; the select that maps it to `-1`
/// is the sign test the bound already performs. Every other key keeps
/// [`normalize`]'s admission (NaN, fractions, `-0`, tags and the
/// `charCodeAt` alias are unchanged).
pub(super) fn normalize_unsigned(ctx: &mut FnCtx<'_>, key: &str, expr: Option<&Expr>) -> String {
    normalize_keyed(ctx, key, expr, unsigned_bound())
}

/// `PERRY_UNSIGNED_BOUND=0`: every element read and store keeps its sign
/// test in front of the bound (N10 off).
pub(crate) fn unsigned_bound() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_UNSIGNED_BOUND").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// What wpfacts proves of the key of an element site (`ElisionFacts::
/// key_signs`, wpfacts `element_keys::collect_signs`): at every copy of the
/// site it is an integer Number inside the int32 range, so its `i32` is the
/// key exactly and the site needs no normalizer ([`exact_i32`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyFact {
    /// Its interval has `lo >= 0`: the key is an array index. The site
    /// needs no sign test and no property-key arm.
    pub(crate) nonneg: bool,
}

/// `PERRY_FACTS_KEY_SIGN=0` (`off`, `false`): no element site consumes its
/// key's interval; every site keeps its normalizer and its sign test.
pub(crate) fn key_sign_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_KEY_SIGN").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// THE query for the key of the element site at `element_offset` (a read or
/// a store of this module). A b1 call's synthesized read carries the body
/// module's offset (`FnCtx::rest_b1`): this module's facts say nothing of
/// its key.
pub(crate) fn key_fact(ctx: &FnCtx<'_>, element_offset: u32) -> Option<KeyFact> {
    if element_offset == 0 || ctx.wpfacts_off || !key_sign_enabled() {
        return None;
    }
    if matches!(ctx.rest_b1, Some((o, _)) if o == element_offset) {
        return None;
    }
    let &(lo, hi) = ctx.wpfacts.key_signs.get(&element_offset)?;
    (lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX)).then_some(KeyFact { nonneg: lo >= 0 })
}

/// [`key_fact`] for the element store `e` (`a[k] = v` as an `IndexSet`).
pub(crate) fn store_key_fact(ctx: &FnCtx<'_>, e: &Expr) -> Option<KeyFact> {
    match e {
        Expr::IndexSet { element_offset, .. } => key_fact(ctx, *element_offset),
        _ => None,
    }
}

/// The `i32` of the key `key` (a double) under `fact`: the native integer it
/// was widened from, else one `fptosi` (exact: the key is an integer inside
/// the int32 range; `-0` is index 0, as its property key `"0"` is). Check
/// mode (`PERRY_VERIFY_FACTS=1`) tests the claim first ([`witness_sign`]).
pub(crate) fn exact_i32(ctx: &mut FnCtx<'_>, key: &str, fact: KeyFact) -> String {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::KeySign,
        || serde_json::json!({ "nonneg": fact.nonneg }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        witness_sign(ctx, key, fact);
    }
    let source = ctx
        .func
        .blocks()
        .iter()
        .rev()
        .find_map(|b| b.widened_i32_source(key));
    source.unwrap_or_else(|| ctx.block().fptosi(DOUBLE, key, I32))
}

/// Check mode: the key `key` is an integer Number inside the int32 range
/// and, under `fact.nonneg`, not negative. A miss reports the false proof
/// ("key sign") and exits 86.
fn witness_sign(ctx: &mut FnCtx<'_>, key: &str, fact: KeyFact) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::KeySign,
        || serde_json::json!({ "nonneg": fact.nonneg }),
    );
    let blk = ctx.block();
    let low = if fact.nonneg { "0.0" } else { "-2147483648.0" };
    let ge = blk.fcmp("oge", key, low);
    let le = blk.fcmp("ole", key, "2147483647.0");
    let inside = blk.and(I1, &ge, &le);
    // Sanitize before `fptosi`: a rejected conversion must not poison.
    let safe = blk.select(I1, &inside, DOUBLE, key, "0.5");
    let int = blk.fptosi(DOUBLE, &safe, I32);
    let back = blk.sitofp(I32, &int, DOUBLE);
    let exact = blk.fcmp("oeq", &back, key);
    let ok = blk.and(I1, &inside, &exact);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("keysign.verify_miss");
    let next = ctx.new_block("keysign.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let what = if fact.nonneg {
        "an element key that is no non-negative int32"
    } else {
        "an element key that is no int32"
    };
    let site = crate::verify_facts::site_arg(ctx, what, crate::verify_facts::Fact::KeySign, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, key)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

fn normalize_keyed(ctx: &mut FnCtx<'_>, key: &str, expr: Option<&Expr>, wrap: bool) -> String {
    let source = ctx
        .func
        .blocks()
        .iter()
        .rev()
        .find_map(|b| b.widened_i32_source(key));
    if let Some(index) = source {
        if wrap {
            return index;
        }
        let valid = ctx.block().icmp_sge(I32, &index, "0");
        return ctx.block().select(I1, &valid, I32, &index, "-1");
    }
    if expr.is_some_and(|e| is_char_code(ctx, e)) {
        // The ordered comparison rejects the sole non-integer result, NaN.
        // A modified String (or Object) prototype may have replaced
        // `charCodeAt` with a function returning anything: `Pristine(String)`
        // must hold too, else the key takes the exact generic admission.
        // Sanitize BEFORE fptosi: even a rejected conversion must not poison.
        let patched = crate::builtin_proto::emit_patched_test(
            ctx,
            crate::builtin_proto::BuiltinProto::String,
        );
        let ordered = ctx.block().fcmp("ord", key, key);
        let pristine = ctx.block().xor(I1, &patched, "true");
        let valid = ctx.block().and(I1, &ordered, &pristine);
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
    let ge = b.fcmp("oge", "%key", "0.0");
    let le = b.fcmp("ole", "%key", "2147483647.0");
    let range = b.and(I1, &ge, &le);
    b.cond_br(&range, "convert.1", "reject.2");

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
#[path = "index_key_sign_tests.rs"]
mod sign_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile_module, CompileOptions};
    use perry_hir::types::Type;
    use perry_hir::{BinaryOp, Function, Module, Param, Stmt};

    fn read_ir(index: Expr, prelude: Vec<Stmt>) -> String {
        let mut module = Module::new("intkey.ts");
        let mut body = prelude;
        body.push(Stmt::Return(Some(Expr::IndexGet {
            element_offset: 0,
            byte_offset: 0,
            byte_end: 0,
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
        // The runtime-key arm hands the original key to the shared cold read
        // (#119da3bb1); the retired helper may still be declared, so assert
        // the call inside that block, not the symbol's presence.
        let runtime_block = ir
            .split_once("\naidx.runtime_key.")
            .map(|(_, rest)| rest.split("\n\n").next().unwrap_or(rest))
            .unwrap_or_else(|| panic!("no runtime-key block: {ir}"));
        assert!(
            runtime_block.contains("call double @perry_array_read_cold_shared("),
            "{runtime_block}"
        );
        assert!(
            !ir.contains("call double @js_array_get_index_or_string("),
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
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
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
    fn hoisted_char_code_var_is_admitted_by_its_typed_locals_fact() {
        // `var c` hoisted (seed `undefined`), then `var c = "abc".charCodeAt(k)`:
        // the redeclaration lowers through LocalSet, which drops the
        // lowering-order alias, so only the whole-body fact admits it.
        let code = Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::String("abc".into())),
                property: "charCodeAt".into(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(2)],
            type_args: vec![],
            byte_offset: 0,
        };
        let var = |init| Stmt::Let {
            id: 4,
            name: "c".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(init),
        };
        let ir = read_ir(Expr::LocalGet(4), vec![var(Expr::Undefined), var(code)]);
        assert!(
            !ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        // NaN and a modified String prototype both take the exact admission.
        assert!(
            ir.contains("fcmp ord double") && ir.contains("llvm.expect.i1"),
            "{ir}"
        );
    }
    #[test]
    fn reassigned_char_code_is_a_negative_control_for_provenance() {
        let code = Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
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
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
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
                    element_offset: 0,
                    byte_offset: 0,
                    byte_end: 0,
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
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
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
    fn literal_choice_index(other: Expr) -> String {
        let choice = Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(Expr::Integer(1)),
            else_expr: Box::new(other),
        };
        let index = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(Expr::Logical {
                op: perry_hir::LogicalOp::Coalesce,
                left: Box::new(choice),
                right: Box::new(Expr::Integer(0)),
            }),
        };
        read_ir(
            Expr::LocalGet(4),
            vec![
                Stmt::Let {
                    id: 5,
                    name: "origin".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Binary {
                        op: BinaryOp::BitAnd,
                        left: Box::new(Expr::NumberCoerce(Box::new(Expr::LocalGet(2)))),
                        right: Box::new(Expr::Integer(127)),
                    }),
                },
                Stmt::Let {
                    id: 4,
                    name: "square".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(index),
                },
            ],
        )
    }

    #[test]
    fn literal_selection_carries_numeric_and_index_proofs_through_a_const_add() {
        let ir = literal_choice_index(Expr::Integer(17));
        assert!(
            !ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(
            !ir.contains("call double @js_dynamic_string_or_number_add("),
            "{ir}"
        );
        assert!(ir.contains("fadd double") || ir.contains("add i32"), "{ir}");
    }

    #[test]
    fn literal_selection_keeps_fractional_nan_overflow_and_dynamic_key_fallbacks() {
        for value in [
            Expr::Number(0.5),
            Expr::Number(f64::NAN),
            Expr::Integer(1i64 << 40),
            Expr::LocalGet(2),
        ] {
            let ir = literal_choice_index(value);
            assert!(
                ir.contains("call i32 @perry_array_index_normalize_i32"),
                "{ir}"
            );
        }
    }

    /// `key | 0`: a native i32 widened to the key's double.
    fn int_key() -> Expr {
        Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(Expr::NumberCoerce(Box::new(Expr::LocalGet(2)))),
            right: Box::new(Expr::Integer(0)),
        }
    }

    /// N10: a widened native key enters the bounded tier as it is. No
    /// select maps it to -1 and the split does not branch on its sign; the
    /// tier's miss arm sends a negative key to the runtime-key arm.
    #[test]
    fn widened_key_read_has_its_sign_test_on_the_miss_arm_only() {
        let ir = read_ir(int_key(), vec![]);
        assert!(!ir.contains(", i32 -1"), "the -1 select is gone: {ir}");
        let entry = ir
            .split_once("define double @")
            .and_then(|(_, f)| f.split("\naidx.canonical.").next())
            .unwrap_or_else(|| panic!("no split: {ir}"));
        assert!(!entry.contains("icmp sge i32"), "{entry}");
        // The miss arm's exit: a negative key to the runtime-key arm, the
        // rest to the tier's cold read.
        let exit = ir
            .lines()
            .find(|l| l.contains("label %aidx.runtime_key.") && l.contains("label %ubound.cold."))
            .unwrap_or_else(|| panic!("no negative exit: {ir}"));
        let cond = exit
            .split_whitespace()
            .nth(2)
            .map(|c| c.trim_end_matches(','))
            .unwrap_or_else(|| panic!("{exit}"));
        assert!(
            ir.contains(&format!("{cond} = icmp slt i32")),
            "the exit tests the sign: {ir}"
        );
    }

    /// N10, negative controls: a key the normalizer admits still declines
    /// NaN, fractions and tags as -1 (now behind the unsigned bound), and
    /// the runtime-key arm keeps the original double.
    #[test]
    fn number_key_keeps_its_normalizer_and_the_exact_runtime_arm() {
        let ir = read_ir(Expr::LocalGet(2), vec![]);
        assert!(
            ir.contains("call i32 @perry_array_index_normalize_i32"),
            "{ir}"
        );
        assert!(ir.contains("ubound.cold"), "{ir}");
        let runtime_block = ir
            .split_once("\naidx.runtime_key.")
            .map(|(_, rest)| rest.split("\n\n").next().unwrap_or(rest))
            .unwrap_or_else(|| panic!("no runtime-key block: {ir}"));
        // Mode 0: the original key, the exact property-key read.
        assert!(
            runtime_block.contains("call double @perry_array_read_cold_shared(")
                && runtime_block.contains(", i64 0)"),
            "{runtime_block}"
        );
    }

    /// N10 on the store side: the guarded store's one bound is
    /// `idx <u capacity`, so a widened key is handed over unselected.
    #[test]
    fn widened_key_store_has_no_sign_select() {
        let store = Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(int_key()),
            value: Box::new(Expr::Number(1.0)),
            byte_offset: 0,
            byte_end: 0,
            element_offset: 0,
        });
        let ir = read_ir(Expr::Integer(0), vec![store]);
        assert!(!ir.contains(", i32 -1"), "{ir}");
        assert!(ir.contains("icmp ult i32"), "{ir}");
    }
}
