//! One ShapeId proof per receiver per straight-line run of statements.
//!
//! In `const i = a.size++; a.left[i] = l; a.right[i] = r;` every read of `a`'s
//! fields re-proves `a`'s ShapeId. A read whose site compares against the
//! class's predicted ShapeId (`class_field_predicted_shape`) proves the same
//! fact as every other such read of `a`. Nothing between them can change that
//! shape without a call: their inline paths read slots, write a Number into an
//! existing raw-f64 lane, or write raw typed-array bytes. So a run of such
//! statements carries the proof in a flag:
//!
//! * the run starts by clearing the flag;
//! * the receiver's first site in the run runs the full site and, when its
//!   compare passes, sets the flag;
//! * a later site tests the flag first: set, it goes straight to its slot
//!   load; clear, it runs its whole site (compare, then the site's miss);
//! * every call the run emits clears the flag before entering the callee (the
//!   call-emission choke point, `LlBlock::dirty_revalidations_before_call`),
//!   so after any slow path, user code or collection later sites run whole.
//!
//! The flag is a stack slot that mem2reg turns into SSA, so on the hit path
//! the reuse costs one branch. It is not a side table: it lives in one
//! activation and is consumed by the same activation's guards. A run admits
//! only straight-line statements whose expressions are reads, local writes,
//! arithmetic, class field updates and declared typed-array element stores
//! (`IndexSet`, or the `PutValueSet` that `index_set::lower` serves); anything
//! else (a conditional or logical operator, another property store, a call, a
//! nested statement list, a closure) ends the run. A flag is set only by a
//! passing compare of its own run and cleared by every call, so a site that
//! finds it set is proven whatever path reached it.
//!
//! Native runs also share the receiver handle as a `ptr addrspace(1)`.
//! The pointer remains in that domain from its first successful receiver
//! test through its last slot load; mem2reg promotes the home and RS4GC
//! computes its live set at every collecting call and emits `gc.relocate`.
//! Calls revoke the shape proof, not the pointer's liveness. The full site
//! after a call derives its receiver afresh and replaces the managed handle
//! before publishing a new proof. The handle derives from a tagged receiver
//! root; its decoded offset is minus the handle floor. RS4GC preserves that
//! base/derived pair and the collector applies the same movement to both.
//! No biased integer crosses a statepoint,
//! and no runtime root, scanner, or shadow slot is added.
use super::FnCtx;
use perry_hir::{Expr, Stmt};
use std::collections::BTreeMap;

pub(crate) struct Window {
    list: usize,
    next: usize,
    active: bool,
    /// Per receiver local: the predicted-ShapeId flag, and whether an earlier
    /// site of the run already sets it.
    slots: BTreeMap<u32, Receiver>,
}

struct Receiver {
    shape: String,
    shape_set: bool,
    handle: Option<String>,
}

/// What a predicted read site of a run receiver does with the run's flag.
pub(crate) struct Memo {
    pub(crate) flag: String,
    pub(crate) reuse: bool,
    /// A native managed pointer, continuously live throughout this run.
    pub(crate) handle: Option<String>,
}

impl Memo {
    pub(crate) fn publish_receiver(&self, blk: &mut crate::block::LlBlock, bits: &str) {
        if let Some(slot) = &self.handle {
            // Native base roots are tagged JSValues. The actual handle is
            // derived from that base; after decoding tags its relocation
            // offset is minus the handle floor, under the bounded signed
            // base/derived GC contract.
            let base = blk.fresh_reg();
            blk.emit_raw(format!("{base} = inttoptr i64 {bits} to ptr addrspace(1)"));
            let handle = blk.fresh_reg();
            blk.emit_raw(format!(
                "{handle} = getelementptr i8, ptr addrspace(1) {base}, i64 {}",
                (super::receiver_range::RECEIVER_BIAS as i64).wrapping_neg()
            ));
            blk.store("ptr addrspace(1)", &handle, slot);
        }
    }

    fn handle_bits(&self, blk: &mut crate::block::LlBlock) -> Option<String> {
        let slot = self.handle.as_ref()?;
        let handle = blk.load("ptr addrspace(1)", slot);
        let bits = blk.fresh_reg();
        // The managed input is continuously live through the run. RS4GC
        // rewrites it before CSE shares this pure identity on call-free hit
        // paths. No integer result is re-rooted, so there is no lifetime hole.
        blk.emit_raw(format!("{bits} = call i64 asm \"\", \"=r,0\"(ptr addrspace(1) {handle}) \"gc-leaf-function\" memory(none)"));
        Some(bits)
    }

    pub(crate) fn field_ptr(&self, blk: &mut crate::block::LlBlock, offset: i64) -> Option<String> {
        let offset = offset + super::receiver_range::HANDLE_FLOOR as i64;
        let bits = self.handle_bits(blk)?;
        let base = blk.inttoptr(crate::types::I64, &bits);
        Some(blk.gep(
            crate::types::I8,
            &base,
            &[(crate::types::I64, &offset.to_string())],
        ))
    }

    pub(crate) fn receiver_test(
        &self,
        blk: &mut crate::block::LlBlock,
        bits: &str,
    ) -> super::receiver_range::FusedReceiver {
        if self.handle.is_none() {
            return super::receiver_range::emit_fused_receiver_test(blk, bits);
        }
        self.publish_receiver(blk, bits);
        let biased = self.handle_bits(blk).unwrap();
        let is_object_pointer = blk.icmp_ult(
            crate::types::I64,
            &biased,
            super::receiver_range::RECEIVER_SPAN_LITERAL,
        );
        super::receiver_range::FusedReceiver {
            biased,
            is_object_pointer,
        }
    }
}

pub(crate) fn memo_for(ctx: &mut FnCtx<'_>, receiver: &Expr) -> Option<Memo> {
    let window = ctx.class_guard_window.as_mut()?;
    if !window.active {
        return None;
    }
    let Expr::LocalGet(id) = receiver else {
        return None;
    };
    let r = window.slots.get_mut(id)?;
    let memo = Memo {
        flag: r.shape.clone(),
        reuse: r.shape_set,
        handle: r.handle.clone(),
    };
    r.shape_set = true;
    Some(memo)
}

/// A receiver whose field accesses take the declared-class guard: a local
/// that is never reassigned or boxed, with a class nominated for it.
fn receiver_local(ctx: &FnCtx<'_>, expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id)
            if !ctx.reassigned_locals.contains(id)
                && !ctx.boxed_vars.contains(id)
                && crate::type_analysis::receiver_class_name(ctx, expr)
                    .or_else(|| {
                        super::property_get::guarded_declared_class_get_candidate(ctx, expr)
                    })
                    .is_some() =>
        {
            Some(*id)
        }
        _ => None,
    }
}

/// Whether `expr`'s inline paths can change no object's shape, counting the
/// guarded class-field sites per receiver local.
fn expr_safe(ctx: &FnCtx<'_>, expr: &Expr, sites: &mut BTreeMap<u32, usize>) -> bool {
    match expr {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::LocalGet(_)
        | Expr::Update { .. } => true,
        Expr::LocalSet(_, value) | Expr::Unary { operand: value, .. } => {
            expr_safe(ctx, value, sites)
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            expr_safe(ctx, left, sites) && expr_safe(ctx, right, sites)
        }
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            if let Some(id) = receiver_local(ctx, object) {
                *sites.entry(id).or_default() += 1;
            }
            expr_safe(ctx, object, sites)
        }
        Expr::IndexGet { object, index, .. } => {
            expr_safe(ctx, object, sites) && expr_safe(ctx, index, sites)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            matches!(target.as_ref(), Expr::PropertyGet { .. })
                && super::typed_element::declared_kind(ctx, target).is_some()
                && super::proxy_reflect::put_value_index_fast_path(ctx, target, key, receiver)
                && expr_safe(ctx, target, sites)
                && expr_safe(ctx, key, sites)
                && expr_safe(ctx, value, sites)
        }
        Expr::IndexSet {
            object,
            index,
            value,
            ..
        } => {
            matches!(object.as_ref(), Expr::PropertyGet { .. })
                && super::typed_element::declared_kind(ctx, object).is_some()
                && expr_safe(ctx, object, sites)
                && expr_safe(ctx, index, sites)
                && expr_safe(ctx, value, sites)
        }
        _ => false,
    }
}

fn stmt_safe(ctx: &FnCtx<'_>, stmt: &Stmt, sites: &mut BTreeMap<u32, usize>) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(init), ..
        } => expr_safe(ctx, init, sites),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) => expr_safe(ctx, expr, sites),
        _ => false,
    }
}

/// Called right before `lower_stmt(stmts[i])`: continue, close or open a run.
pub(crate) fn before_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    let list = stmts.as_ptr() as usize;
    if !stmt_safe(ctx, &stmts[i], &mut BTreeMap::new()) {
        close(ctx);
        return;
    }
    if let Some(window) = ctx.class_guard_window.as_mut() {
        if window.list == list && window.next == i {
            window.next = i + 1;
            window.active = true;
            return;
        }
    }
    close(ctx);
    let mut sites = BTreeMap::new();
    for stmt in &stmts[i..] {
        if !stmt_safe(ctx, stmt, &mut sites) {
            break;
        }
    }
    let mut slots = BTreeMap::new();
    for (id, count) in sites {
        if count < 2 {
            continue;
        }
        let shape = ctx.func.alloca_entry(crate::types::I8);
        ctx.func
            .entry_allocas_push_store(crate::types::I8, "0", &shape);
        ctx.block().store(crate::types::I8, "0", &shape);
        ctx.func.reg_counter().push_guard_memo_slot(shape.clone());
        // Keep the handle as a managed derivation of a tagged receiver root.
        // mem2reg + RS4GC carry its base/derived relocation
        // across any miss call. There is no root lifetime hole inside a run;
        // unlike boxed root reloads this does not require identity laundering.
        let handle = crate::codegen::helpers::native_stack_roots_enabled().then(|| {
            let slot = ctx.func.alloca_entry("ptr addrspace(1)");
            ctx.func
                .entry_allocas_push_store("ptr addrspace(1)", "null", &slot);
            slot
        });
        slots.insert(
            id,
            Receiver {
                shape,
                shape_set: false,
                handle,
            },
        );
    }
    if slots.is_empty() {
        return;
    }
    ctx.class_guard_window = Some(Window {
        list,
        next: i + 1,
        active: true,
        slots,
    });
}

/// Called right after `lower_stmt`: guards outside a run's own statements
/// never consult it.
pub(crate) fn after_stmt(ctx: &mut FnCtx<'_>) {
    if let Some(window) = ctx.class_guard_window.as_mut() {
        window.active = false;
    }
}

/// End the run: later calls no longer clear its flags.
pub(crate) fn close(ctx: &mut FnCtx<'_>) {
    if ctx.class_guard_window.take().is_some() {
        ctx.func.reg_counter().clear_guard_memo_slots();
    }
}

#[cfg(all(test, feature = "llvm-inprocess"))]
mod tests {
    use super::*;
    use crate::types::{I64, VOID};

    #[test]
    fn shared_handle_is_relocated_across_a_collecting_call() {
        let _native = crate::testing::NativeRootsPin::native();
        let mut module = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
        module.declare_function("js_gc", VOID, &[]);
        let f = module.define_function("memo_gc", I64, vec![(I64, "%handle".into())]);
        f.enable_shadow_frame(1);
        let slot = f.alloca_entry("ptr addrspace(1)");
        f.entry_allocas_push_store("ptr addrspace(1)", "null", &slot);
        let memo = Memo {
            flag: String::new(),
            reuse: false,
            handle: Some(slot),
        };
        let b = f.create_block("entry");
        memo.publish_receiver(b, "%handle");
        b.call_void("js_gc", &[]);
        let ptr = memo.field_ptr(b, 32).unwrap();
        let value = b.load(I64, &ptr);
        b.ret(I64, &value);
        let ir = module.to_ir();
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&ir, "x86_64-unknown-linux-gnu", "memo_gc")
                .expect("managed receiver must survive the statepoint verifier");
        assert!(
            rewritten
                .lines()
                .any(|line| line.contains("call ")
                    && line.contains("@llvm.experimental.gc.relocate")),
            "a handle used after collection must be relocated: {rewritten}"
        );
        assert!(
            rewritten.contains("\"gc-live\"(ptr addrspace(1)"),
            "the shared handle must be in the collecting call's live set: {rewritten}"
        );
        // Negative control: the same emitter's integer/plain-pointer form
        // has no managed operand for the collecting call to relocate.
        let control = ir
            .replace("ptr addrspace(1)", "ptr")
            .replace("addrspacecast ptr", "bitcast ptr");
        let unrooted = crate::inprocess::statepoint_rewritten_ir(
            &control,
            "x86_64-unknown-linux-gnu",
            "memo_gc_negative_control",
        )
        .expect("negative-control IR must remain valid");
        assert!(
            !unrooted
                .lines()
                .any(|line| line.contains("call ")
                    && line.contains("@llvm.experimental.gc.relocate")),
            "the negative control unexpectedly retained a managed root: {unrooted}"
        );
    }
}
