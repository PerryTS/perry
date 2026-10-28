//! Fast/slow splits of re-entering runtime helpers (deferred-collection RFC,
//! step S2).
//!
//! A call that can collect is an RS4GC statepoint: every GC value live across
//! it is spilled before the call and reloaded after it. Several helpers can
//! collect only on a rare arm — a getter, a setter, a throw, an allocating
//! miss — yet every call paid that cost. Each lowering here puts the common
//! case on a path that is either no call at all or a call to an audited
//! `CannotCollect` leaf (`gc_call_effects.rs`, which is what makes it
//! `"gc-leaf-function"`), and keeps the original collecting call on a cold
//! arm. RS4GC then places `gc.relocate`s only at that cold statepoint.
//!
//! Every value the cold arm passes is computed before the fast test, so it
//! dominates the arm; the arm rejoins through a phi.

use super::FnCtx;
use crate::types::{LlvmType, DOUBLE, I1, I64};

/// `fast(fast_args)`; when it answers `TAG_HOLE`, a cold arm calls
/// `slow(slow_args)`. Returns the joined DOUBLE.
///
/// `fast` must be a `CannotCollect` symbol whose contract is "serve the hit,
/// `TAG_HOLE` for anything else", and `slow` must reproduce the full helper
/// for every case `fast` declines.
pub(crate) fn emit_hole_declining_split(
    ctx: &mut FnCtx<'_>,
    tag: &str,
    fast: &str,
    fast_args: &[(LlvmType, &str)],
    slow: &str,
    slow_args: &[(LlvmType, &str)],
) -> String {
    let slow_idx = ctx.new_block(&format!("{tag}.split_slow"));
    let merge_idx = ctx.new_block(&format!("{tag}.split_merge"));
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let fast_val = ctx.block().call(DOUBLE, fast, fast_args);
    let fast_bits = ctx.block().bitcast_double_to_i64(&fast_val);
    let served = ctx
        .block()
        .icmp_ne(I64, &fast_bits, crate::nanbox::TAG_HOLE_I64);
    let fast_end = ctx.block().label.clone();
    // The SERVED edge is the true edge, like every guard-passing edge in the
    // property towers (#7883).
    ctx.block().cond_br(&served, &merge_label, &slow_label);

    ctx.current_block = slow_idx;
    let slow_val = {
        let _decision = crate::fallback_diag::decision(
            &format!("non-collecting helper {fast} returned TAG_HOLE; general operation required"),
            &format!("TAG_HOLE decline from {fast}"),
        );
        ctx.block().call(DOUBLE, slow, slow_args)
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (fast_val.as_str(), fast_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    )
}

/// `` `${v}` `` for an operand that is already a string (heap or SSO) is the
/// operand itself — `js_template_string_coerce_box`'s second short-circuit.
/// That arm is now inline and calls nothing; every other operand (numbers,
/// which allocate their text, and objects, whose `toString` is user code)
/// keeps the call on a cold arm.
pub(crate) fn emit_template_string_coerce(ctx: &mut FnCtx<'_>, v: &str) -> String {
    let slow_idx = ctx.new_block("tmpl_coerce.slow");
    let merge_idx = ctx.new_block("tmpl_coerce.merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let entry_end = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(v);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_heap = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
        let is_sso = blk.icmp_eq(I64, &top16, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
        let is_str = blk.or(I1, &is_heap, &is_sso);
        let end = blk.label.clone();
        blk.cond_br(&is_str, &merge_label, &slow_label);
        end
    };

    ctx.current_block = slow_idx;
    let coerced = ctx
        .block()
        .call(DOUBLE, "js_template_string_coerce_box", &[(DOUBLE, v)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (v, entry_end.as_str()),
            (coerced.as_str(), slow_end.as_str()),
        ],
    )
}

/// The checked dynamic-callee unbox (#5504) with its hit inline: a
/// `POINTER_TAG` value unboxes to its low 48 bits, which is everything
/// `js_closure_unbox_callee_checked` does on that arm. Every other value
/// takes the call on a cold arm, where it throws `TypeError: value is not a
/// function` exactly as before.
pub(crate) fn emit_checked_callee_unbox(ctx: &mut FnCtx<'_>, callee: &str) -> String {
    let slow_idx = ctx.new_block("callee_unbox.slow");
    let merge_idx = ctx.new_block("callee_unbox.merge");
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let (handle, entry_end) = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(callee);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_ptr = blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let end = blk.label.clone();
        blk.cond_br(&is_ptr, &merge_label, &slow_label);
        (handle, end)
    };

    ctx.current_block = slow_idx;
    let checked = ctx
        .block()
        .call(I64, "js_closure_unbox_callee_checked", &[(DOUBLE, callee)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[
            (handle.as_str(), entry_end.as_str()),
            (checked.as_str(), slow_end.as_str()),
        ],
    )
}

/// Does a fixed-name read of `property` own a shape/slot memo? Only a name
/// some getter of the program is declared under can reach the collecting
/// exit the memo serves; any other read keeps the stateless probe and owns
/// no storage. A compile without the program's accessor names gives every
/// read its memo.
pub(crate) fn read_may_reach_getter(ctx: &FnCtx<'_>, property: &str) -> bool {
    ctx.program_class_accessor_names
        .as_ref()
        .map_or(true, |names| names.may_read_through_getter(property))
}

/// Shared atomic storage for a site's shape/slot answers. No heap pointer or
/// validity state is stored, so these zero-filled integer words need no root.
pub(crate) fn emit_read_site_memo(ctx: &mut FnCtx<'_>, words: usize) -> String {
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let name = format!(
        "@{}_read_shape_slots",
        crate::expr::inline_cache_global_name(ctx, site)
    );
    ctx.typed_parse_rodata.push(format!(
        "{name} = private global [{words} x i64] zeroinitializer, align 8"
    ));
    name
}

#[cfg(test)]
mod tests {

    #[test]
    fn read_site_symbol_key_owns_no_short_string_memo() {
        use perry_hir::{Expr, Module, Stmt};
        let mut module = Module::new("read_site_symbol.ts");
        module.init.push(Stmt::Expr(Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::GlobalGet(100)),
            index: Box::new(Expr::SymbolFor(Box::new(Expr::String("key".into())))),
        }));
        let ir = String::from_utf8(
            crate::compile_module(
                &module,
                crate::CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!ir.contains("_read_shape_slots = private"), "{ir}");
        assert!(ir.contains("call double @perry_probe_get_keyed("), "{ir}");
        assert!(
            ir.contains("call double @js_object_get_keyed_slow("),
            "{ir}"
        );
        assert!(
            !ir.contains("call double @js_object_get_keyed_site_slow("),
            "{ir}"
        );
    }
    #[test]
    fn only_a_declared_getter_name_owns_read_site_storage() {
        use perry_hir::{Expr, Module, Stmt};
        for (getter, memo) in [("width", true), ("height", false)] {
            let mut module = Module::new("read_site_names.ts");
            module.init.push(Stmt::Expr(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::GlobalGet(100)),
                property: "width".into(),
                byte_offset: 0,
            }));
            let names = crate::codegen::ClassAccessorNames::from_names(
                [getter.to_string()],
                std::iter::empty::<String>(),
            );
            let ir = String::from_utf8(
                crate::compile_module(
                    &module,
                    crate::CompileOptions {
                        emit_ir_only: true,
                        program_class_accessor_names: Some(std::sync::Arc::new(names)),
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(ir.contains("_read_shape_slots = private"), memo, "{ir}");
            assert_eq!(
                ir.contains("call double @perry_probe_get_memo("),
                memo,
                "{ir}"
            );
            // Declarations name every entrance; only the calls differ.
            let calls = |callee: &str| {
                ir.lines()
                    .any(|l| !l.starts_with("declare") && l.contains(&format!("@{callee}(")))
            };
            assert_eq!(calls("js_object_get_field_value_site"), memo, "{ir}");
            assert_eq!(calls("perry_probe_get"), !memo, "{ir}");
            assert_eq!(calls("js_object_get_field_value"), !memo, "{ir}");
        }
    }

    #[test]
    fn read_site_storage_never_needs_tls_initialization() {
        use perry_hir::{Expr, Module, Stmt};
        for (worker, thread_agent) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut module = Module::new("read_site_agents.ts");
            module.init.push(Stmt::Expr(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::GlobalGet(100)),
                property: "width".into(),
                byte_offset: 0,
            }));
            let ir = String::from_utf8(
                crate::compile_module(
                    &module,
                    crate::CompileOptions {
                        emit_ir_only: true,
                        program_has_worker: worker,
                        program_has_thread_agents: thread_agent,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap();
            assert!(ir.contains("call double @perry_probe_get_memo("), "{ir}");
            let declaration = ir
                .lines()
                .find(|line| line.contains("_read_shape_slots = private"))
                .expect("a generic read must own its shape/slot storage");
            assert!(!declaration.contains("thread_local"), "{declaration}");
        }
    }
}
