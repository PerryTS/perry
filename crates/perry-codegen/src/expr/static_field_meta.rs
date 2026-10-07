//! StaticFieldGet..NativeModuleRef (class meta + getters).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::rooting::{
    any_operand_may_collect, with_rooted_accumulator, with_rooted_group, Arg, Repr,
};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{emit_root_nanbox_store_for_expr, lower_expr, nanbox_pointer_inline, FnCtx};

/// Initialize a class evaluation's lexical self-binding `owner` with the
/// evaluated class value `class_box`.
pub(crate) fn store_evaluation_owner(
    ctx: &mut FnCtx<'_>,
    owner: perry_hir::types::LocalId,
    class_box: &str,
) {
    let Some(slot) = ctx.locals.get(&owner).cloned() else {
        return;
    };
    if matches!(
        ctx.local_type_hint(&owner),
        Some(perry_hir::types::Type::Array(_))
    ) {
        // Shared-mutable capture rewriting turns a self binding captured by
        // its own methods into a one-element cell. Initialize the cell's
        // value, preserving the handle stored in the local slot.
        let cell_box = ctx.block().load(DOUBLE, &slot);
        let cell_bits = ctx.block().bitcast_double_to_i64(&cell_box);
        let cell = ctx
            .block()
            .and(I64, &cell_bits, crate::nanbox::POINTER_MASK_I64);
        ctx.block().call_void(
            "js_array_set_f64",
            &[(I64, &cell), (I32, "0"), (DOUBLE, class_box)],
        );
    } else {
        ctx.block().store(DOUBLE, class_box, &slot);
    }
}

/// The compiled symbols for `template`'s `static { … }` blocks, in declaration
/// order (#685).
///
/// Lifted out of `Expr::ClassExprFresh`'s body so the #7154 rooting predicate
/// can ask "does this class expression run arbitrary user code?" *before* the
/// object is exposed, instead of discovering it at the loop that invokes them.
fn static_block_fns(ctx: &FnCtx<'_>, template: &str) -> Vec<String> {
    ctx.classes
        .get(template)
        .map(|c| {
            c.static_methods
                .iter()
                .filter(|m| m.name.starts_with("__perry_static_init_"))
                .filter_map(|m| {
                    ctx.methods
                        .get(&(
                            template.to_string(),
                            crate::codegen::static_method_registry_key(&m.name),
                        ))
                        .cloned()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The interned name bytes of a static field, as (`@bytes`, len).
fn static_field_name_bytes(ctx: &mut FnCtx<'_>, field_name: &str) -> (String, String) {
    let idx = ctx.strings.intern(field_name);
    let entry = ctx.strings.entry(idx);
    (
        format!("@{}", entry.bytes_global),
        entry.byte_len.to_string(),
    )
}

/// `C.x` through its compiled alias, unless the alias is detached
/// (`TAG_HOLE` — see `class_static_alias_sync`): then the generic [[Get]].
fn emit_detached_static_get(
    ctx: &mut FnCtx<'_>,
    class_id: u32,
    field_name: &str,
    value: &str,
) -> String {
    let bits = ctx.block().bitcast_double_to_i64(value);
    let detached = ctx
        .block()
        .icmp_eq(crate::types::I64, &bits, crate::nanbox::TAG_HOLE_I64);
    let from_l = ctx.block_label(ctx.current_block);
    let slow_idx = ctx.new_block("staticget.detached");
    let join_idx = ctx.new_block("staticget.join");
    let slow_l = ctx.block_label(slow_idx);
    let join_l = ctx.block_label(join_idx);
    ctx.block().cond_br(&detached, &slow_l, &join_l);
    ctx.current_block = slow_idx;
    let (bytes, len) = static_field_name_bytes(ctx, field_name);
    let slow = ctx.block().call(
        DOUBLE,
        "js_class_static_field_get",
        &[
            (crate::types::I32, &(class_id as i32).to_string()),
            (PTR, &bytes),
            (crate::types::I64, &len),
        ],
    );
    let slow_end_l = ctx.block_label(ctx.current_block);
    ctx.block().br(&join_l);
    ctx.current_block = join_idx;
    ctx.block()
        .phi(DOUBLE, &[(value, &from_l), (&slow, &slow_end_l)])
}

/// `C.x = v`: when the compiled alias is detached, the generic [[Set]] runs in
/// its own block and joins; the caller emits the attached store in the
/// current block and then branches to the returned join block.
fn emit_detached_static_put(
    ctx: &mut FnCtx<'_>,
    class_id: u32,
    field_name: &str,
    global_name: &str,
    value: &str,
) -> usize {
    let current = ctx.block().load(DOUBLE, &format!("@{global_name}"));
    let bits = ctx.block().bitcast_double_to_i64(&current);
    let detached = ctx
        .block()
        .icmp_eq(crate::types::I64, &bits, crate::nanbox::TAG_HOLE_I64);
    let slow_idx = ctx.new_block("staticset.detached");
    let fast_idx = ctx.new_block("staticset.attached");
    let join_idx = ctx.new_block("staticset.join");
    let slow_l = ctx.block_label(slow_idx);
    let fast_l = ctx.block_label(fast_idx);
    let join_l = ctx.block_label(join_idx);
    ctx.block().cond_br(&detached, &slow_l, &fast_l);
    ctx.current_block = slow_idx;
    let (bytes, len) = static_field_name_bytes(ctx, field_name);
    ctx.block().call_void(
        "js_class_static_field_put",
        &[
            (crate::types::I32, &(class_id as i32).to_string()),
            (PTR, &bytes),
            (crate::types::I64, &len),
            (DOUBLE, value),
        ],
    );
    ctx.block().br(&join_l);
    ctx.current_block = fast_idx;
    join_idx
}

fn private_static_storage_name(class_id: u32, field_name: &str) -> String {
    format!("#<perry:private-value:{class_id}:{field_name}>")
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => {
            if field_name.starts_with('#') {
                let class_id = ctx.class_ids.get(class_name).copied().unwrap_or(0);
                let object = Expr::PrivateGuard {
                    class_name: class_name.clone(),
                    class_id,
                    field_name: field_name.clone(),
                    kind: 0,
                    op: 2,
                    receiver_is_brand_owner: false,
                    object: Box::new(Expr::ClassRef(class_name.clone())),
                };
                return lower_expr(
                    ctx,
                    &Expr::PropertyGet {
                        object: Box::new(object),
                        property: private_static_storage_name(class_id, field_name),
                        byte_offset: 0,
                    },
                );
            }
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                let value = ctx.block().load(DOUBLE, &g_ref);
                match ctx.class_ids.get(class_name).copied() {
                    Some(class_id) if !field_name.starts_with('#') => {
                        Ok(emit_detached_static_get(ctx, class_id, field_name, &value))
                    }
                    _ => Ok(value),
                }
            } else {
                Ok(double_literal(0.0))
            }
        }
        Expr::StaticFieldSet {
            class_name,
            field_name,
            value,
        } => {
            let v = lower_expr(ctx, value)?;
            let key = (class_name.clone(), field_name.clone());
            let global_name = ctx.static_field_globals.get(&key).cloned();
            // A detached alias (`TAG_HOLE`: deleted / accessor / read-only)
            // takes the generic [[Set]] instead of the direct store below.
            let join_idx = match (global_name.as_ref(), ctx.class_ids.get(class_name).copied()) {
                (Some(global_name), Some(class_id)) if !field_name.starts_with('#') => Some(
                    emit_detached_static_put(ctx, class_id, field_name, global_name, &v),
                ),
                _ => None,
            };
            if let Some(global_name) = global_name
                .as_ref()
                .filter(|_| !field_name.starts_with('#'))
            {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): static field global slot is registered as a mutable GC root
                // (register_module_globals_as_gc_roots walks ctx.static_field_globals since the
                // 2026-07-02 audit fix; before that this comment was aspirational and the slot
                // was unrooted).
                emit_root_nanbox_store_for_expr(ctx, &v, &g_ref, value);
            }
            // v0.5.747: also register the static field in the runtime
            // CLASS_DYNAMIC_PROPS side-table so dynamic-dispatch reads
            // (when the class ref is in an Any-typed local) find it.
            // Refs #420 / #618 followup.
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let runtime_field_name = if field_name.starts_with('#') {
                    private_static_storage_name(class_id, field_name)
                } else {
                    field_name.clone()
                };
                let idx = ctx.strings.intern(&runtime_field_name);
                let entry = ctx.strings.entry(idx);
                let bytes_ref = format!("@{}", entry.bytes_global);
                let len_str = entry.byte_len.to_string();
                let cid_str = class_id.to_string();
                let global_slot = global_name
                    .as_ref()
                    .filter(|_| !field_name.starts_with('#'))
                    .map(|name| format!("@{name}"))
                    .unwrap_or_else(|| "null".to_string());
                // A static private element is defined as one at its source
                // (an `ENTRY_PRIVATE` key of the class function, #11791).
                let register = if field_name.starts_with('#') {
                    "js_class_register_static_private_field"
                } else {
                    "js_class_register_static_field"
                };
                ctx.block().call_void(
                    register,
                    &[
                        (crate::types::I32, &cid_str),
                        (crate::types::PTR, &bytes_ref),
                        (crate::types::I64, &len_str),
                        (DOUBLE, &v),
                        (PTR, &global_slot),
                    ],
                );
            }
            if let Some(join_idx) = join_idx {
                let join_l = ctx.block_label(join_idx);
                ctx.block().br(&join_l);
                ctx.current_block = join_idx;
            }
            Ok(v)
        }
        // Issue #711: dynamic parent-class registration for `class X
        // extends fn(...)` shapes. Evaluate the extends expression and
        // call `js_register_class_parent_dynamic(child_cid, value)`,
        // which extracts a class_id from the value (ClassRef payload
        // for INT32-tagged, ObjectHeader.class_id for POINTER-tagged)
        // and wires the parent edge into CLASS_REGISTRY. No-op if the
        // value carries no class_id — preserves the parentless
        // baseline rather than throwing during module init.
        Expr::RegisterClassParentDynamic {
            class_name,
            parent_expr,
        } => {
            let val = lower_expr(ctx, parent_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_register_class_parent_dynamic",
                        &[(crate::types::I32, &cid_str), (DOUBLE, &val)],
                    );
                }
            }
            // Yield undefined — this expression is always wrapped in
            // `Stmt::Expr` for its side effect; the return value isn't
            // observable to user code.
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Snapshot a function-nested class's captured outer locals into the
        // runtime CLASS_CAPTURE_VALUES table at the decl site, so DYNAMIC
        // construction of the class value (`exports.C = C; new mod.C()` —
        // the webpack/zod bundle pattern) can fill the synthesized
        // `__perry_cap_<id>` ctor params. Mirrors RegisterClassParentDynamic
        // placement; static `new C()` sites pass captures inline and never
        // consult the table.
        Expr::RegisterClassCaptures {
            class_name,
            captures,
        } => {
            // #6052: the snapshot refresh emitted after EACH captured var's
            // assignment (#6037) can legally read a SIBLING capture whose
            // `let`/`const` has not run yet (`const _fs = ..; <refresh>;
            // const _path = ..` — the SWC CJS interop shape). Those loads are
            // Perry-internal materialization, not user reads: bracket them in
            // a TDZ-suppression window so a dead-zone box snapshots as
            // `undefined` (a later refresh fixes it up) instead of throwing
            // the #6044 ReferenceError. The window holds only these
            // side-effect-free capture loads — no user code runs inside it.
            ctx.block().call_void("js_tdz_suppress_begin", &[]);
            let mut lowered: Vec<String> = Vec::with_capacity(captures.len());
            for (index, c) in captures.iter().enumerate() {
                let v = lower_expr(ctx, c)?;
                // A class-environment class's evaluation publishes here too.
                super::class_env::store_class_env_slot(ctx, class_name, index as u32, &v, c);
                lowered.push(v);
            }
            ctx.block().call_void("js_tdz_suppress_end", &[]);
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 && !lowered.is_empty() {
                    let n = lowered.len();
                    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                    for (i, v) in lowered.iter().enumerate() {
                        let slot =
                            ctx.block()
                                .gep(DOUBLE, &buf, &[(crate::types::I64, &i.to_string())]);
                        ctx.block().store(DOUBLE, v, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf
                    ));
                    let cid_str = class_id.to_string();
                    let len_str = n.to_string();
                    ctx.block().call_void(
                        "js_class_register_capture_values",
                        &[
                            (crate::types::I32, &cid_str),
                            (crate::types::PTR, &ptr_reg),
                            (crate::types::I64, &len_str),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // #6654: refresh one evaluated class expression's OWN capture array.
        // The old end-of-body refresh wrote the shared template-name snapshot,
        // so `make("b")` could backfill stale slots on the class object returned
        // by an earlier `make("a")`. Build the replacement array first, then
        // reload the rooted owner local and let the runtime safely no-op when
        // this control-flow path never evaluated the class expression.
        Expr::RefreshClassExprCaptures {
            class_value,
            captures,
            env_class,
        } => {
            let cap_len = captures.len().to_string();
            // The capture array is live across every capture's lowering, and a
            // capture can collect (a property read through an IC miss, a
            // getter): then it is an accumulator in ONE root slot, re-read by
            // each push and republished with the push's result (the array may
            // grow). A refresh whose captures cannot collect — the common case,
            // plain local and boxed-variable reads — has no collection point
            // between two pushes, so it emits no slot at all: a per-push slot
            // costs seven blocks, and a module-scope closure holding several
            // hundred class refreshes of ~75 captures each grew by ~370k blocks,
            // which made LLVM's mem2reg quadratic (tsc compiled 3-4x slower).
            let protect = crate::rooting::any_operand_may_collect(ctx, captures.iter());
            let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_len)]);
            let caps_arr = crate::rooting::with_rooted_accumulator(
                ctx,
                crate::rooting::Repr::Ptr,
                &arr,
                protect,
                |ctx, acc| {
                    ctx.block().call_void("js_tdz_suppress_begin", &[]);
                    for (index, capture) in captures.iter().enumerate() {
                        let value = lower_expr(ctx, capture)?;
                        if let Some(env_class) = env_class {
                            super::class_env::store_class_env_slot(
                                ctx,
                                env_class,
                                index as u32,
                                &value,
                                capture,
                            );
                        }
                        acc.advance(
                            ctx,
                            "js_array_push_f64",
                            &[crate::rooting::Arg::Plain(DOUBLE, &value)],
                        );
                    }
                    Ok(())
                },
                |_, current| Ok(current.to_string()),
            )?;
            ctx.block().call_void("js_tdz_suppress_end", &[]);
            let caps_box = nanbox_pointer_inline(ctx.block(), &caps_arr);
            // Lower after the allocating array operations so a movable class
            // object is reloaded from its compiler-private rooted local.
            let owner = lower_expr(ctx, class_value)?;
            if let Some(env_class) = env_class {
                super::class_env::publish_guarded(
                    ctx,
                    env_class,
                    "js_class_env_refresh",
                    &owner,
                    &caps_box,
                );
            }
            let key_idx = ctx.strings.intern("__perry_ctor_caps");
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let key_box = ctx.block().load(DOUBLE, &key_handle_global);
            let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
            let key_raw = ctx
                .block()
                .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
            ctx.block().call_void(
                "js_class_object_refresh_capture_values",
                &[(DOUBLE, &owner), (I64, &key_raw), (DOUBLE, &caps_box)],
            );
            Ok(owner)
        }
        // Read slot `index` of the class's decl-site capture snapshot —
        // STATIC method prologue rebinds (no instance to carry the
        // `__perry_cap_*` fields).
        Expr::ClassCaptureValue {
            class_name,
            index,
            fallback,
            prefer_fallback,
        } => {
            // The fallback (a synthesized `__perry_cap_*` ctor param) must be
            // lowered FIRST so its value is available regardless of whether the
            // class id resolves — and so its side-effect-free read happens in
            // program order.
            let fallback_v = match fallback {
                Some(fb) => Some(lower_expr(ctx, fb)?),
                None => None,
            };
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let cid_str = class_id.to_string();
                let idx_str = index.to_string();
                return Ok(match (&fallback_v, *prefer_fallback) {
                    // #5437 (param-first): the LIVE param (the `new`-site cap
                    // arg) wins whenever present; the decl-site snapshot is
                    // consulted ONLY when the param is `undefined` (the
                    // cross-module construct path drops the cap arg). Used by the
                    // synthesized constructor's capture rebind so a SAME-module
                    // `new C(...)`'s current (possibly mutated) outer is NOT
                    // overridden by a stale snapshot.
                    (Some(fb), true) => ctx.block().call(
                        DOUBLE,
                        "js_param_or_class_capture_value",
                        &[
                            (DOUBLE, fb),
                            (crate::types::I32, &cid_str),
                            (crate::types::I32, &idx_str),
                        ],
                    ),
                    // #5437: snapshot-or-fallback (snapshot-first). The decl-site
                    // snapshot wins when it holds a real value (W6: the appended
                    // cap arg may be a mis-boxed multi-level capture, or —
                    // cross-module — absent entirely); otherwise the fallback is
                    // used.
                    (Some(fb), false) => ctx.block().call(
                        DOUBLE,
                        "js_class_capture_value_or",
                        &[
                            (crate::types::I32, &cid_str),
                            (crate::types::I32, &idx_str),
                            (DOUBLE, fb),
                        ],
                    ),
                    (None, _) => {
                        let receiver = if let Some(this_slot) = ctx.this_stack.last().cloned() {
                            ctx.block().load(DOUBLE, &this_slot)
                        } else {
                            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                        };
                        ctx.block().call(
                            DOUBLE,
                            "js_class_capture_value_for_receiver",
                            &[
                                (DOUBLE, &receiver),
                                (crate::types::I32, &cid_str),
                                (crate::types::I32, &idx_str),
                            ],
                        )
                    }
                });
            }
            // Class id unknown in this module: keep the fallback if we have one
            // (the cap param the construct supplied), else `undefined`.
            Ok(fallback_v.unwrap_or_else(|| double_literal(f64::from_bits(0x7FFC_0000_0000_0001))))
        }
        // Issue #894: `static [Symbol.for("k")] = init` inside a
        // class expression returned from a factory function. Emitted
        // by HIR lowering as a `Sequence([…, RegisterClassStaticSymbol,
        // ClassRef])` so each factory invocation re-registers the
        // (class_id, sym_key) → value entry. Without this, the
        // registration would only happen at module-init time when
        // referenced free variables may not yet be assigned, and
        // `isSchema(C)` (which checks `TypeId in C`) returns false on
        // a freshly-returned class.
        Expr::RegisterClassStaticSymbol {
            class_name,
            key_expr,
            value_expr,
        } => {
            let rooted_operands: [&perry_hir::Expr; 2] = [key_expr, value_expr];
            let (rooted_values, rooted_group) =
                crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
            let key_v = rooted_values[0].clone();
            let val_v = rooted_values[1].clone();
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_class_register_static_symbol",
                        &[
                            (crate::types::I32, &cid_str),
                            (DOUBLE, &key_v),
                            (DOUBLE, &val_v),
                        ],
                    );
                }
            }
            let rooted_result = double_literal(f64::from_bits(0x7FFC_0000_0000_0001));
            rooted_group.release(ctx);
            Ok(rooted_result)
        }
        Expr::RegisterClassComputedMethod {
            class_name,
            key_expr,
            method_name,
            is_static,
            param_count,
            has_rest,
            definition_order,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let registry_name = if *is_static {
                        crate::codegen::static_method_registry_key(method_name)
                    } else {
                        method_name.clone()
                    };
                    if let Some(llvm_name) = ctx.methods.get(&(class_name.clone(), registry_name)) {
                        let func_ref = format!("@{}", llvm_name);
                        let func_i64 = ctx.block().ptrtoint(&func_ref, I64);
                        let cid_str = class_id.to_string();
                        let param_count_str = param_count.to_string();
                        let is_static_str = (*is_static as i64).to_string();
                        let has_rest_str = (*has_rest as i64).to_string();
                        let definition_order_str = definition_order.to_string();
                        // A static one's own function object runs its
                        // closure-convention entry's `JsFunctionInfo` (string pool).
                        let entry_i64 = if *is_static {
                            let info = ctx.block().fn_info_ref(&format!("{llvm_name}__clo"));
                            ctx.block().ptrtoint(&info, I64)
                        } else {
                            "0".to_string()
                        };
                        ctx.block().call_void(
                            "js_register_class_computed_method",
                            &[
                                (I64, &cid_str),
                                (DOUBLE, &key_v),
                                (I64, &func_i64),
                                (I64, &param_count_str),
                                (I64, &is_static_str),
                                (I64, &has_rest_str),
                                (I64, &definition_order_str),
                                (I64, &entry_i64),
                            ],
                        );
                    }
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        Expr::RegisterClassComputedAccessor {
            class_name,
            key_expr,
            getter_name,
            setter_name,
            is_static,
            definition_order,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let getter_i64 = getter_name
                        .as_ref()
                        .and_then(|name| {
                            let registry_name = if *is_static {
                                crate::codegen::static_method_registry_key(name)
                            } else {
                                name.clone()
                            };
                            ctx.methods.get(&(class_name.clone(), registry_name))
                        })
                        .map(|llvm_name| {
                            let func_ref = format!("@{}", llvm_name);
                            ctx.block().ptrtoint(&func_ref, I64)
                        })
                        .unwrap_or_else(|| "0".to_string());
                    let setter_i64 = setter_name
                        .as_ref()
                        .and_then(|name| {
                            let registry_name = if *is_static {
                                crate::codegen::static_method_registry_key(name)
                            } else {
                                name.clone()
                            };
                            ctx.methods.get(&(class_name.clone(), registry_name))
                        })
                        .map(|llvm_name| {
                            let func_ref = format!("@{}", llvm_name);
                            ctx.block().ptrtoint(&func_ref, I64)
                        })
                        .unwrap_or_else(|| "0".to_string());
                    let cid_str = class_id.to_string();
                    let is_static_str = (*is_static as i64).to_string();
                    let definition_order_str = definition_order.to_string();
                    ctx.block().call_void(
                        "js_register_class_computed_accessor",
                        &[
                            (I64, &cid_str),
                            (DOUBLE, &key_v),
                            (I64, &getter_i64),
                            (I64, &setter_i64),
                            (I64, &is_static_str),
                            (I64, &definition_order_str),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Issue #1772: per-evaluation class identity for a class expression.
        // Each evaluation allocates a real heap "class object" — a regular
        // object stamped with the compile-time template's `class_id` (so
        // static methods / `new` / instanceof dispatch through the existing
        // class_id machinery, no fresh hop, no method regression) and
        // carrying the per-evaluation static fields as its own properties.
        // So `make(a) !== make(b)` (distinct pointers), `make(a).ast` is an
        // own field, `make(a).pipe()` dispatches via class_id=template, and
        // the object is GC-traced + collected when unreachable (no leak).
        Expr::ClassExprFresh {
            template,
            evaluation_owner,
            shared_first_evaluation,
            ..
        } => {
            // #11759 (c′): the first evaluation is the shared class; the
            // fresh class object below is every later one.
            if let Some(first_init) = shared_first_evaluation {
                return super::class_first_evaluation::lower(
                    ctx,
                    template,
                    *evaluation_owner,
                    first_init,
                    expr,
                );
            }
            lower_class_evaluation_object(ctx, expr)
        }
        Expr::ClassIsFirstEvaluation { value, template } => {
            super::class_first_evaluation::lower_is_first(ctx, value, template)
        }
        Expr::SetFunctionPrototype {
            func,
            proto,
            strict,
        } => {
            with_rooted_group(ctx, 1, |ctx, group| {
                let protect_receiver = any_operand_may_collect(ctx, [proto.as_ref()]);
                let receiver = group.lower(ctx, func, protect_receiver)?;
                let value = lower_expr(ctx, proto)?;
                let receiver = group.reread(ctx, receiver)?;
                // The setter can run user code and collect. Its rooted return
                // supplies the assignment result after any evacuation.
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_set_prototype_property",
                    &[
                        (DOUBLE, &receiver),
                        (DOUBLE, &value),
                        (I32, if *strict { "1" } else { "0" }),
                    ],
                ))
            })
        }
        // Link a generator/async-generator instance into the spec prototype
        // chain. Closure bodies can use their own closure pointer to preserve
        // `Object.getPrototypeOf(g()) === g.prototype`; direct function bodies
        // fall back to the generic two-hop intrinsic linker and call sites may
        // relink with a concrete closure when they have one.
        Expr::LinkGeneratorPrototype { obj, is_async } => {
            let obj_val = lower_expr(ctx, obj)?;
            if let Some(closure_ptr) = crate::expr::try_current_closure_ptr_value(ctx) {
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_generator_attach_closure_prototype",
                    &[(DOUBLE, &obj_val), (crate::types::I64, &closure_ptr)],
                ));
            }
            let is_async_str = if *is_async { "1" } else { "0" };
            Ok(ctx.block().call(
                DOUBLE,
                "js_generator_attach_prototype",
                &[(DOUBLE, &obj_val), (crate::types::I32, is_async_str)],
            ))
        }
        // Issue #838: `<Class>.prototype.<method> = <fn>` and the
        // aliased `let p = <Class>.prototype; p.<method> = <fn>`
        // shape. HIR recognises the assignment pattern and lowers it
        // here; codegen emits `js_register_prototype_method(class_id,
        // name_ptr, name_len, value)` so the runtime stores the
        // closure into a per-class side-table consulted at dispatch
        // time. The expression yields the closure value to match
        // JS-spec `x.foo = bar`. If the class isn't in
        // `ctx.class_ids` (cross-module imported class) we fall back
        // to a generic field-set on `<Class>.prototype` so the value
        // at least lands on the prototype proxy — the importer side
        // typically owns the registration anyway.
        Expr::RegisterPrototypeMethod {
            class_name,
            method_name,
            value,
        } => {
            let val_double = lower_expr(ctx, value)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let key_idx = ctx.strings.intern(method_name);
                let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
                let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
                ctx.block().call_void(
                    "js_register_prototype_method",
                    &[
                        (crate::types::I32, &class_id.to_string()),
                        (PTR, &key_bytes_global),
                        (I64, &key_len),
                        (DOUBLE, &val_double),
                    ],
                );
            }
            Ok(val_double)
        }
        // Issue #838 followup (b): the prototype's owner is a function
        // declaration (Babel's `var Foo = function(){ function Foo(){…};
        // Foo.prototype.x = fn; return Foo; }()`, also dayjs's minified
        // form). Hand both the closure value and the method name to the
        // runtime helper — it allocates (or reuses) a synthetic class id
        // keyed by the closure's NaN-boxed bits and stores the method
        // on `CLASS_PROTOTYPE_METHODS[synthetic_cid]`. The paired
        // `new <FuncRef>(args)` lowering below stamps the same id on
        // the instance so dispatch finds the method via the regular
        // `(*obj).class_id` walk.
        //
        // #11635: `func` is evaluated FIRST and is live across the lowering
        // of `value`, which is routinely a call (`proto.toIsoString =
        // deprecate(msg, fn)` in moment). Holding it in a register let an
        // evacuating minor inside that call move the closure while the
        // register kept its from-space address, and the runtime then read
        // the retired closure header in `synthetic_class_id_for_function`.
        // Root it across the window and re-read it below. `value` is
        // rooted across the registration call too, because that call is a
        // `Reenters` runtime entry and the value is the expression result.
        Expr::RegisterFunctionPrototypeMethod {
            func,
            method_name,
            value,
        } => with_rooted_group(ctx, 2, |ctx, group| {
            let protect_func = any_operand_may_collect(ctx, [value.as_ref()]);
            let func_i = group.lower(ctx, func, protect_func)?;
            let val_i = group.lower(ctx, value, true)?;
            let func_double = group.reread(ctx, func_i)?;
            let val_double = group.reread(ctx, val_i)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            let _ = ctx.block().call(
                crate::types::I32,
                "js_register_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                    (DOUBLE, &val_double),
                ],
            );
            group.reread(ctx, val_i)
        }),
        // Read side of #838 followup (b): `<funcDecl>.prototype.<name>`
        // (Ident or computed-string-literal form) lowered into a direct
        // lookup of the prototype-method side-table. Returns the closure
        // value stored at registration time, or `undefined` if no method
        // by that name was registered. Pre-fix this would fall through
        // to a generic PropertyGet on a `Function.prototype` object that
        // never materialised (so the read was always `undefined`,
        // making `typeof Foo.prototype.method` come back `'undefined'`
        // even though `(new Foo()).method` correctly reached the
        // registered closure via the dispatch path).
        Expr::GetFunctionPrototypeMethod { func, method_name } => {
            let func_double = lower_expr(ctx, func)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_get_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                ],
            ))
        }
        // `static [Symbol.for("k")] = "v"` — register in the runtime's
        // class-static-symbol side table. Refs #420 (drizzle).
        Expr::ClassStaticSymbolSet {
            class_name,
            key,
            value,
        } => {
            let rooted_operands: [&perry_hir::Expr; 2] = [key, value];
            let (rooted_values, rooted_group) =
                crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
            let key_v = rooted_values[0].clone();
            let val_v = rooted_values[1].clone();
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
            }
            let rooted_result = val_v;
            rooted_group.release(ctx);
            Ok(rooted_result)
        }
        // Issue #894: when `NativeModuleRef` reaches this fallback path
        // (i.e. its parent isn't one of the dedicated fast-paths above —
        // for example it's the *return value* of a CJS-wrap synthesized
        // `require()` call, then stashed in a local and member-accessed
        // later: `const { EventEmitter } = require('node:events')` →
        // `Let { id: 6, init: Call(require, "node:events") }` followed by
        // `Let { id: 7, init: PropertyGet { LocalGet(6), "EventEmitter" } }`),
        // pre-fix the value lowered to the literal `0.0`. `0.0` is plain
        // f64 zero (not the NaN-boxed `undefined` tag), so a subsequent
        // `PropertyGet { LocalGet(6), "X" }` slow-pathed into
        // `js_object_get_field_by_name` with a null receiver and returned
        // `undefined` — and a then-chained `PropertyGet { LocalGet(7),
        // "prototype" }` on that `undefined` tripped the spec
        // "Cannot read properties of undefined (reading 'prototype')"
        // throw (pino's `lib/proto.js` exact shape).
        //
        // Materialize a real NATIVE_MODULE_CLASS_ID-tagged ObjectHeader
        // here so the downstream property-by-name path routes through the
        // namespace's NATIVE_MODULE_CLASS_ID arm in
        // `js_object_get_field_by_name` — that consults
        // `get_native_module_constant` and `is_native_module_callable_export`
        // exactly as the direct-NativeModuleRef fast path does. The two
        // paths now converge: `import * as fs from "node:fs"; fs.constants`
        // (direct AST shape, fast-path at line 3615) and `const fs =
        // require("node:fs"); fs.constants` (call-result shape, fallback
        // path here) both produce a real namespace object.
        Expr::NativeModuleRef(name) => {
            if let Some(submod_key) = crate::nm_install::native_namespace_submodule_key(name) {
                let submod_idx = ctx.strings.intern(submod_key);
                let submod_bytes_global =
                    format!("@{}", ctx.strings.entry(submod_idx).bytes_global);
                let submod_len = submod_key.len().to_string();
                let install_sym = crate::nm_install::nm_submod_install_symbol(submod_key);
                let blk = ctx.block();
                if let Some(symbol) = install_sym {
                    blk.call_void(symbol, &[]);
                }
                return Ok(blk.call(
                    DOUBLE,
                    "js_node_submodule_namespace",
                    &[(PTR, &submod_bytes_global), (I32, &submod_len)],
                ));
            }
            let mod_idx = ctx.strings.intern(name);
            let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
            let mod_len_str = name.len().to_string();
            // Devirt: register this module's dispatch bucket before the namespace
            // exists, so method calls route to it. Sole compile-time ref to the
            // bucket's handlers — unimported modules dead-strip.
            let install_sym = crate::nm_install::nm_install_symbol(name);
            let blk = ctx.block();
            if let Some(s) = install_sym {
                blk.call_void(s, &[]);
            }
            Ok(blk.call(
                DOUBLE,
                "js_create_native_module_namespace",
                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
            ))
        }

        // ObjectRest is the `...rest` capture in destructuring:
        // `const { a, b, ...rest } = obj` — `rest` must be a clone of
        // `obj` with keys `a`/`b` stripped. We build an exclude-keys
        // array of NaN-boxed strings and call `js_object_rest`, which
        // returns a fresh object pointer that we re-NaN-box.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}

/// One evaluation's class object (`Expr::ClassExprFresh`): a fresh heap class
/// object carrying the evaluation's statics, captures and self-binding.
pub(crate) fn lower_class_evaluation_object(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    let Expr::ClassExprFresh {
        template,
        evaluation_owner,
        named_statics,
        computed_keys,
        computed_statics,
        static_init_order,
        captured_args,
        evaluated_parent,
        ..
    } = expr
    else {
        unreachable!("lower_class_evaluation_object takes a ClassExprFresh");
    };
    // #11759 (c′): this evaluation's parent is the evaluated class its
    // binding holds; register it for `js_class_evaluation_object` to pin, as
    // a runtime heritage value does.
    if let Some(parent) = evaluated_parent {
        lower_expr(
            ctx,
            &Expr::RegisterClassParentDynamic {
                class_name: template.clone(),
                parent_expr: parent.clone(),
            },
        )?;
    }
    let template_cid = ctx.class_ids.get(template).copied().unwrap_or(0);
    let tcid_str = template_cid.to_string();
    // The template's own record (its template cell), which the module's
    // string-pool initializer defines for every template it evaluates.
    let cell = if template_cid == 0 {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::codegen::fresh_class_templates::template_cell_global(template_cid)
        )
    };
    // Room for the evaluation's template key, its own `length`, `name`
    // and static methods, its pinned parent, its captured environment and
    // its prototype object besides its static fields, so the template's
    // shapes are all inline slots (`class_object_template`).
    let own_member_slots =
        4 + ctx.classes.get(template).map_or(0, |c| {
            c.static_methods.len()
                + usize::from(c.extends_expr.is_some() || evaluated_parent.is_some())
        }) + usize::from(!captured_args.is_empty());
    let nfields = (named_statics.len() + own_member_slots).to_string();
    // A static FIELD named `length` / `name` takes over the intrinsic
    // property; it is created as an ordinary one.
    let field_mask = named_statics
        .iter()
        .fold(0u32, |mask, (name, _)| match name.as_str() {
            "length" => mask | 1,
            "name" => mask | 2,
            _ => mask,
        });
    // #1789 / #6438: the evaluation's class object, stamped with
    // class_id = template and marked a class object
    // (ShapeObjectKind::Class, so `typeof` reports "function" and
    // `new`/`instanceof` read the class_id from it), owning its
    // `length`, `name` and static methods and pinned to THIS
    // evaluation's parent. The lowering sequences
    // `RegisterClassParentDynamic` immediately ahead of this node, so
    // `CLASS_DYNAMIC_PARENT_VALUE[template]` still holds that parent;
    // later evaluations overwrite it, but each object keeps its own
    // edge. Every evaluation after the template's first is allocated
    // directly in the template's final shape.
    let obj = ctx.block().call(
        I64,
        "js_class_evaluation_object",
        &[
            (I32, &tcid_str),
            (I32, &nfields),
            (I32, &field_mask.to_string()),
            (PTR, &cell),
        ],
    );
    // #7154: the fresh class object is a raw SSA register while the
    // evaluation allocates (every static initializer and captured
    // argument), and an evacuating minor relocates it, after which each
    // later `js_object_set_field_by_name` would write into from-space —
    // the statics would land on the abandoned copy. So the object is
    // always held in a rooted group (`Expr::Object`'s contract since
    // #6951), re-read before each use. (`CLASS_OBJECT_VALUES` does not
    // protect the register: it is a forwarded root that keeps the
    // OBJECT alive but never rewrites `%obj`.)
    let block_fns = static_block_fns(ctx, template);
    let protect_handle = true;
    with_rooted_group(ctx, 1, |ctx, group| {
        let rooted = group.adopt_emitted(ctx, Repr::Ptr, &obj, protect_handle);
        // A named class expression's lexical self-binding is
        // initialized immediately after the class value is created,
        // before computed names and static initializers run. The
        // compiler-private owner let was emitted at body entry, so its
        // slot is already shadow-bound and remains a GC root while the
        // initializer sequence allocates.
        if let Some(owner) = evaluation_owner {
            let obj = group.reread_emitted(ctx, rooted);
            let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
            store_evaluation_owner(ctx, *owner, &obj_box);
        }
        // Resolve all ComputedPropertyNames before any static field
        // initializer, preserving class-body order. Hidden own slots
        // carry the resulting PropertyKeys for both the static phase
        // below and later instance construction.
        for (name, key_expr) in computed_keys {
            let key_value = lower_expr(ctx, key_expr)?;
            let key_idx = ctx.strings.intern(name);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let obj = group.reread_emitted(ctx, rooted);
            let blk = ctx.block();
            let storage_key = blk.load(DOUBLE, &key_handle_global);
            let storage_bits = blk.bitcast_double_to_i64(&storage_key);
            let storage_raw = blk.and(I64, &storage_bits, crate::nanbox::POINTER_MASK_I64);
            blk.call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj), (I64, &storage_raw), (DOUBLE, &key_value)],
            );
        }
        // #1787: snapshot the captured outer-scope values onto the class
        // object as the `__perry_ctor_caps` own array (in the constructor's
        // capture-param order). `new <thisClassObjectValue>()` reads it back
        // in `js_new_function_construct` and replays the constructor with the
        // right captured environment — which the static `new ClassName()`
        // inlining can't do once the class escapes its defining scope.
        if !captured_args.is_empty() {
            let cap_len = captured_args.len().to_string();
            let caps_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_len)]);
            // #7615 slice 8: the capture snapshot is an ACCUMULATOR — the
            // array holds the only reference to everything pushed so far
            // while the next element is lowered — and it was threaded
            // through a bare SSA register, which is #6951's shape exactly.
            //
            // The window is EMPTY on today's HIR, and the flag says so
            // rather than the code assuming it: `captured_args` is built at
            // exactly one site (`lower/lower_expr/arm_class.rs`) as
            // `ids.iter().map(|id| Expr::LocalGet(*id))`, and
            // `expr_may_trigger_gc` answers `false` for every `LocalGet`.
            // So `protect_caps` is false today, `advance` threads the same
            // register the old code threaded, and the emitted IR is byte
            // for byte what it was. What changes is that the day a
            // non-inert expression reaches this list it is rooted by
            // construction instead of silently entering the window.
            let protect_caps = any_operand_may_collect(ctx, captured_args.iter());
            // #6523: these capture loads are Perry-internal materialization
            // at the class's DEFINITION site, same as the
            // `RegisterClassCaptures` snapshot loads above (#6052). A
            // captured `const` declared AFTER the class (bundled semver's
            // `class Comparator` + trailing debug/require consts) is still
            // in its dead zone here — legal JS, since TDZ applies at
            // method-call time. Without the suppression window the checked
            // box read threw "Cannot access undefined before
            // initialization" while merely DEFINING the class. Suppressed
            // loads snapshot `undefined`; the #6037 refresh statements
            // re-register the live values right after each captured
            // refresh the evaluated object's array right after each
            // captured binding's initializer runs.
            let caps_box = with_rooted_accumulator(
                ctx,
                Repr::Ptr,
                &caps_arr,
                protect_caps,
                |ctx, acc| {
                    ctx.block().call_void("js_tdz_suppress_begin", &[]);
                    for (index, arg) in captured_args.iter().enumerate() {
                        let v = lower_expr(ctx, arg)?;
                        // The evaluation publishes a class-environment
                        // class's slots before any static initializer
                        // or member can read them.
                        super::class_env::store_class_env_slot(
                            ctx,
                            template,
                            index as u32,
                            &v,
                            arg,
                        );
                        acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v)]);
                    }
                    ctx.block().call_void("js_tdz_suppress_end", &[]);
                    Ok(())
                },
                |ctx, arr| Ok(nanbox_pointer_inline(ctx.block(), arr)),
            )?;
            // #7154: re-read the class object — the capture lowerings above
            // are arbitrary expressions and may have moved it.
            let obj = group.reread_emitted(ctx, rooted);
            // Its own `__perry_ctor_caps`: one recorded transition from
            // the template's final shape (`class_object_template`).
            ctx.block().call_void(
                "js_class_object_set_ctor_caps",
                &[(I64, &obj), (DOUBLE, &caps_box), (PTR, &cell)],
            );
            // A guarded class environment learns this evaluation; the
            // first one publishes its captures into the slots.
            let obj = group.reread_emitted(ctx, rooted);
            let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
            super::class_env::publish_guarded(
                ctx,
                template,
                "js_class_env_evaluate",
                &obj_box,
                &caps_box,
            );
        }
        // Static fields and blocks execute only after every computed
        // name has been resolved, then in their original ClassBody
        // order. Each vector index is recorded by HIR lowering.
        for step in static_init_order {
            match step {
                perry_hir::ClassFreshStaticInit::Named(index) => {
                    let Some((name, init)) = named_statics.get(*index as usize) else {
                        continue;
                    };
                    let storage_name = if name.starts_with('#') {
                        private_static_storage_name(template_cid, name)
                    } else {
                        name.clone()
                    };
                    let key_idx = ctx.strings.intern(&storage_name);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let value = lower_expr(ctx, init)?;
                    let obj = group.reread_emitted(ctx, rooted);
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                    // A static private element is claimed as one where it is
                    // created (#11791).
                    let define = if name.starts_with('#') {
                        "js_class_object_define_static_private"
                    } else {
                        "js_object_set_field_by_name"
                    };
                    blk.call_void(define, &[(I64, &obj), (I64, &key_raw), (DOUBLE, &value)]);
                }
                perry_hir::ClassFreshStaticInit::Computed(index) => {
                    let Some((key_slot, init)) = computed_statics.get(*index as usize) else {
                        continue;
                    };
                    let value = lower_expr(ctx, init)?;
                    let key_idx = ctx.strings.intern(key_slot);
                    let entry = ctx.strings.entry(key_idx);
                    let key_bytes = format!("@{}", entry.bytes_global);
                    let key_len = entry.byte_len.to_string();
                    let obj = group.reread_emitted(ctx, rooted);
                    let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
                    let resolved_key = ctx.block().call(
                        DOUBLE,
                        "js_object_get_own_field_or_undef",
                        &[(DOUBLE, &obj_box), (PTR, &key_bytes), (I64, &key_len)],
                    );
                    ctx.block().call(
                        DOUBLE,
                        "js_object_set_property_key",
                        &[
                            (DOUBLE, &obj_box),
                            (DOUBLE, &resolved_key),
                            (DOUBLE, &value),
                        ],
                    );
                }
                perry_hir::ClassFreshStaticInit::Block(index) => {
                    let Some(fn_name) = block_fns.get(*index as usize) else {
                        continue;
                    };
                    let obj = group.reread_emitted(ctx, rooted);
                    let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
                    ctx.block()
                        .call_void("js_static_this_arm_value", &[(DOUBLE, &obj_box)]);
                    ctx.block().call(DOUBLE, fn_name, &[]);
                }
            }
        }
        let obj = group.reread_emitted(ctx, rooted);
        let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
        Ok(obj_box)
    })
}
