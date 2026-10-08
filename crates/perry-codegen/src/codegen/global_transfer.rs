//! Immutable module-global transfer plans for perry/thread agents.
//!
//! perry/thread shares user-module globals and module-once initialization
//! across agents that each own a separate moving heap. A String/BigInt the
//! initializer stored in `@perry_global_<prefix>__<id>` is an object of the
//! initializing agent's arena: that agent's collector rewrites the canonical
//! slot, never the address another agent loaded from it. So no agent but the
//! owner may read a heap leaf out of the canonical slot.
//!
//! For each eligible binding this module generates, in the producer module:
//!
//! * `@perry_gpub_<prefix>__<id>`: the process-wide publication cell. The
//!   owning initializer publishes a pointer-free record (String bytes + flags,
//!   or BigInt limbs) into it exactly once (`js_thread_global_publish`).
//! * entry `slot` of `@perry_gtc_<prefix>`, the producer module's per-agent
//!   block: one thread-local `[N x { double value, i64 state }]` for all of the
//!   module's eligible bindings. An entry's value word is registered as the
//!   reading agent's own GC root before first use; a cold read materializes an
//!   ordinary movable leaf there (`js_thread_global_materialize`).
//! * `perry_gread_<prefix>__<id>`: the producer's accessor, used by exported
//!   getters and namespace wrappers. Generated code in the producer module
//!   emits the same hot path inline (`emit_read`).
//!
//! Every access to the block goes through ONE address per function
//! invocation (`LlFunction::entry_tls_address`, `llvm.threadlocal.address` at
//! the top of the entry block), and each binding is a fixed offset from it. A
//! hit is therefore two loads and a compare: no `tlv_get_addr` call per read on
//! Darwin, no per-iteration `fs:` address on x86_64, and one address per
//! function however many of the module's bindings it reads.
//!
//! The canonical slot and the module-once initializer are unchanged. The plan
//! is compiler metadata keyed by the binding's LocalId in its producer module;
//! there is no runtime registry or lookup by heap address. Eligibility reuses
//! the module-global emitter's initializer count and module-wide reassignment
//! analysis; a TypeScript annotation never establishes a leaf.
//!
//! Scope: programs that launch perry/thread agents and construct no
//! `worker_threads` Worker (those evaluate modules per thread; a process-wide
//! cell would merge independent evaluations). Programs without perry/thread
//! agents emit nothing here.

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::types::{DOUBLE, I64, PTR};

/// LLVM type of one binding's entry in the per-agent block: value at offset
/// 0, state at offset 8
/// (`perry-runtime/src/thread/global_transfer.rs::AgentGlobalCache`).
pub(crate) const CACHE_TY: &str = "{ double, i64 }";
/// An entry's initial value: `undefined`, state Unseen.
const CACHE_INIT: &str = "{ double 0x7FFC000000000001, i64 0 }";
/// Must match the runtime's `STATE_LOCAL_LEAF_READY` / `STATE_CANONICAL_NONLEAF`.
const STATE_LOCAL_LEAF_READY: &str = "2";
const STATE_CANONICAL_NONLEAF: &str = "3";
const MATERIALIZE: &str = "js_thread_global_materialize";
const PUBLISH: &str = "js_thread_global_publish";

/// Generated storage and accessor symbols of one eligible binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GlobalTransfer {
    /// The unchanged canonical slot, without `@`.
    pub canonical: String,
    pub cell: String,
    /// The producer module's per-agent block, without `@`.
    pub agent_block: String,
    /// This binding's entry in `agent_block`.
    pub slot: u32,
    pub accessor: String,
}

impl GlobalTransfer {
    /// `slot` is the binding's position among the module's eligible bindings,
    /// in initializer order.
    pub(crate) fn new(module_prefix: &str, id: u32, slot: u32) -> Self {
        Self {
            canonical: format!("perry_global_{module_prefix}__{id}"),
            cell: format!("perry_gpub_{module_prefix}__{id}"),
            agent_block: format!("perry_gtc_{module_prefix}"),
            slot,
            accessor: format!("perry_gread_{module_prefix}__{id}"),
        }
    }
}

/// This agent's entry for `t`, from the function's one block address.
fn cache_entry(
    func: &mut crate::function::LlFunction,
    block_idx: usize,
    t: &GlobalTransfer,
) -> String {
    let base = func.entry_tls_address(&t.agent_block);
    let slot = t.slot.to_string();
    func.block_mut(block_idx)
        .expect("current block exists")
        .gep_inbounds(CACHE_TY, &base, &[(I64, &slot)])
}

/// Whether the transfer applies to this module's compilation: perry/thread
/// agents can run its code and no Worker evaluates modules per thread.
pub(crate) fn enabled(thread_agents: bool, workers: bool) -> bool {
    thread_agents && !workers
}

/// Can a single-assignment binding with this initializer hold a heap String or
/// BigInt? `proven` is the structural runtime kind (never an annotation).
/// Unknown kinds stay candidates; the owner's runtime tag decides at
/// publication. Only constructs that structurally allocate something else are
/// excluded, so their reads keep the canonical route with no added work.
pub(crate) fn may_hold_leaf(
    init: &perry_hir::Expr,
    proven: Option<&perry_hir::types::Type>,
) -> bool {
    use perry_hir::types::Type;
    use perry_hir::Expr;
    match proven {
        Some(Type::String) | Some(Type::BigInt) => return true,
        Some(_) => return false,
        None => {}
    }
    !matches!(
        init,
        Expr::Object(_)
            | Expr::Array(_)
            | Expr::Closure { .. }
            | Expr::New { .. }
            | Expr::FuncRef(_)
            | Expr::ExternFuncRef { .. }
            | Expr::ClassRef(_)
    )
}

/// Emit the module's per-agent block, and each binding's publication cell and
/// producer accessor. `transfers` holds every eligible binding of the module,
/// `transfers[i].slot == i`.
pub(crate) fn emit_storage(llmod: &mut LlModule, transfers: &[GlobalTransfer]) {
    let Some(first) = transfers.first() else {
        return;
    };
    debug_assert!(transfers
        .iter()
        .enumerate()
        .all(|(i, t)| t.slot as usize == i && t.agent_block == first.agent_block));
    llmod.declare_function(crate::function::TLS_ADDRESS_INTRINSIC, PTR, &[PTR]);
    // Per agent: one entry per binding.
    let entries = vec![format!("{CACHE_TY} {CACHE_INIT}"); transfers.len()].join(", ");
    llmod.add_internal_thread_local_global(
        &first.agent_block,
        &format!("[{} x {CACHE_TY}]", transfers.len()),
        &format!("[{entries}]"),
    );
    for t in transfers {
        // Process-wide: written once by the owning initializer, read by every agent.
        llmod.add_internal_global(&t.cell, I64, "0");
        emit_accessor(llmod, t);
    }
}

fn emit_accessor(llmod: &mut LlModule, t: &GlobalTransfer) {
    let f = llmod.define_function(&t.accessor, DOUBLE, vec![]);
    let _ = f.create_block("entry");
    let _ = f.create_block("gread.ready");
    let _ = f.create_block("gread.check");
    let _ = f.create_block("gread.canonical");
    let _ = f.create_block("gread.cold");
    let label =
        |f: &mut crate::function::LlFunction, i: usize| f.block_mut(i).unwrap().label.clone();
    let (ready, check, canonical, cold) = (label(f, 1), label(f, 2), label(f, 3), label(f, 4));
    let entry = cache_entry(f, 0, t);
    let blk = f.block_mut(0).unwrap();
    let state_ptr = blk.gep_inbounds(
        CACHE_TY,
        &entry,
        &[(crate::types::I32, "0"), (crate::types::I32, "1")],
    );
    let state = blk.load(I64, &state_ptr);
    let is_ready = blk.icmp_eq(I64, &state, STATE_LOCAL_LEAF_READY);
    blk.cond_br(&is_ready, &ready, &check);
    let blk = f.block_mut(1).unwrap();
    let v = blk.load(DOUBLE, &entry);
    blk.ret(DOUBLE, &v);
    let blk = f.block_mut(2).unwrap();
    let is_nonleaf = blk.icmp_eq(I64, &state, STATE_CANONICAL_NONLEAF);
    blk.cond_br(&is_nonleaf, &canonical, &cold);
    let blk = f.block_mut(3).unwrap();
    let v = blk.load(DOUBLE, &format!("@{}", t.canonical));
    blk.ret(DOUBLE, &v);
    let blk = f.block_mut(4).unwrap();
    let args = cold_args(blk, t, &entry);
    let v = blk.call(
        DOUBLE,
        MATERIALIZE,
        &[(I64, &args[0]), (I64, &args[1]), (I64, &args[2])],
    );
    blk.ret(DOUBLE, &v);
}

fn cold_args(blk: &mut crate::block::LlBlock, t: &GlobalTransfer, entry: &str) -> [String; 3] {
    [
        blk.ptrtoint(&format!("@{}", t.cell), I64),
        blk.ptrtoint(entry, I64),
        blk.ptrtoint(&format!("@{}", t.canonical), I64),
    ]
}

/// The ONE read of an eligible binding in generated code: the current agent's
/// TLS cache when its leaf is ready, the canonical slot for a published nonleaf,
/// otherwise the cold materializer. The hit path is a TLS address, a state
/// compare and a load: no call, atomic, lock or foreign-heap read. The TLS
/// address is the function's one entry-block address of the module's block.
pub(crate) fn emit_read(ctx: &mut FnCtx<'_>, t: &GlobalTransfer) -> String {
    let cache = cache_entry(ctx.func, ctx.current_block, t);
    let ready_idx = ctx.new_block("gread.ready");
    let check_idx = ctx.new_block("gread.check");
    let canonical_idx = ctx.new_block("gread.canonical");
    let cold_idx = ctx.new_block("gread.cold");
    let merge_idx = ctx.new_block("gread.merge");
    let ready_label = ctx.block_label(ready_idx);
    let check_label = ctx.block_label(check_idx);
    let canonical_label = ctx.block_label(canonical_idx);
    let cold_label = ctx.block_label(cold_idx);
    let merge_label = ctx.block_label(merge_idx);

    let blk = ctx.block();
    let state_ptr = blk.gep_inbounds(
        CACHE_TY,
        &cache,
        &[(crate::types::I32, "0"), (crate::types::I32, "1")],
    );
    let state = blk.load(I64, &state_ptr);
    let is_ready = blk.icmp_eq(I64, &state, STATE_LOCAL_LEAF_READY);
    blk.cond_br(&is_ready, &ready_label, &check_label);

    ctx.current_block = ready_idx;
    let ready_value = ctx.block().load(DOUBLE, &cache);
    ctx.block().br(&merge_label);

    ctx.current_block = check_idx;
    let is_nonleaf = ctx.block().icmp_eq(I64, &state, STATE_CANONICAL_NONLEAF);
    ctx.block()
        .cond_br(&is_nonleaf, &canonical_label, &cold_label);

    ctx.current_block = canonical_idx;
    let canonical_value = ctx.block().load(DOUBLE, &format!("@{}", t.canonical));
    ctx.block().br(&merge_label);

    ctx.current_block = cold_idx;
    let args = cold_args(ctx.block(), t, &cache);
    let cold_value = ctx.block().call(
        DOUBLE,
        MATERIALIZE,
        &[(I64, &args[0]), (I64, &args[1]), (I64, &args[2])],
    );
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (&ready_value, &ready_label),
            (&canonical_value, &canonical_label),
            (&cold_value, &cold_end),
        ],
    )
}

/// Read a module global of the module being compiled: the transfer route for
/// an eligible binding, otherwise the canonical slot as before.
pub(crate) fn load_module_global(ctx: &mut FnCtx<'_>, id: u32, global_name: &str) -> String {
    if let Some(t) = ctx.module_global_transfers.get(&id).cloned() {
        debug_assert_eq!(t.canonical, global_name);
        return emit_read(ctx, &t);
    }
    ctx.block().load(DOUBLE, &format!("@{global_name}"))
}

/// Publish right after the owning initializer stored the binding's value into
/// its registered canonical slot. The hook reads the value from that root, so
/// no SSA value is carried into it, and it never collects.
pub(crate) fn emit_publish(ctx: &mut FnCtx<'_>, id: u32) {
    let Some(t) = ctx.module_global_transfers.get(&id).cloned() else {
        return;
    };
    if ctx.block().is_terminated() {
        // The initializer completed abruptly: the binding stays pending.
        return;
    }
    let entry = cache_entry(ctx.func, ctx.current_block, &t);
    let args = cold_args(ctx.block(), &t, &entry);
    ctx.block().call_void(
        PUBLISH,
        &[(I64, &args[0]), (I64, &args[1]), (I64, &args[2])],
    );
}

#[cfg(test)]
#[path = "global_transfer_tests.rs"]
mod tests;
