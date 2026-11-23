//! Read a per-agent runtime pointer from emitted code, without a call where
//! the target allows it (`perry-runtime/src/agent_ptrs.rs`).
//!
//! The runtime keeps a small per-agent block of pointers,
//! `PERRY_AGENT_PTRS` (a `#[thread_local]` array of [`AGENT_PTR_SLOTS`]
//! pointers). How emitted code reaches it depends on the target:
//!
//! * [`AgentPtrAccess::ExecutableTls`] — ELF executables (Linux; x86-64 and
//!   aarch64): the block is an `external thread_local` global in
//!   the static TLS block of the executable the runtime archive is linked
//!   into, which the module addresses local-exec
//!   ([`crate::module::LlModule::use_local_exec_tls`]): the access is one
//!   thread-pointer-relative load (`mov %fs:PERRY_AGENT_PTRS@TPOFF+off` on
//!   x86-64, which folds into a consuming compare; `mrs tpidr_el0` + a
//!   `:tprel:` offset on aarch64). Not used for dylib/staticlib outputs, nor
//!   for Android and OpenHarmony programs, which are always shared objects:
//!   an image that can be `dlopen`ed has no fixed offset in the static TLS
//!   block.
//! * [`AgentPtrAccess::AppleTsd`] — Apple aarch64: Mach-O has no initial-exec
//!   model (every thread-local access is a TLV thunk call), so the block's
//!   address is read from the runtime's `HotTls` cache through the pthread
//!   TSD fast path (`hot_tls.rs`), at `HOT_TLS_AGENT_PTRS_OFFSET`.
//! * [`AgentPtrAccess::WindowsTeb`] — Windows x86-64, any output kind: the
//!   same sequence the compiler emits for a native thread-local, spelled out
//!   because the runtime cannot export the block under a stable name there
//!   (`agent_ptrs.rs`): `gs:[0x58]` (the TEB's `ThreadLocalStoragePointer`)
//!   indexed by the image's `_tls_index` gives this thread's TLS block for
//!   the image, and the block sits at `PERRY_AGENT_PTRS_SECREL` (a `.secrel32`
//!   the runtime emits for its own static) inside it. Emitted code and the
//!   runtime are linked into ONE image, so `_tls_index` is theirs.
//! * [`AgentPtrAccess::Call`] — everything else (wasm, arm64_32, Windows
//!   aarch64, x86-64 Darwin, ELF dylib/staticlib outputs, Android and
//!   OpenHarmony): the runtime
//!   accessor. x86-64 Darwin has no call-free thread-local model (every
//!   Mach-O thread-local access is a TLV thunk call) and the runtime's
//!   pthread-TSD fast path (`HotTls`, the Apple aarch64 route) is built for
//!   aarch64 only.
//!
//! In every inline form a null slot means "not published yet" and takes the
//! accessor call, which publishes it; so the inline forms and the call are
//! equivalent by construction.
//!
//! ## One block address per activation
//!
//! The block's ADDRESS is a per-thread constant (a `#[thread_local]` with no
//! destructor, valid for the thread's whole life); only the slots' CONTENTS
//! change (a directory or prototype published later). So a site reads the
//! address its activation computed once and loads the slot itself:
//!
//! * ELF executables: one `llvm.threadlocal.address` in the entry block
//!   ([`crate::function::LlFunction::entry_tls_address`]), shared by every
//!   site of the function.
//! * Apple aarch64: a body with a start ([`begin_activation`]: functions, closures,
//!   methods and module top-level code) reads it once there, through the TSD path, or through the
//!   thread-local itself when the direct path is unavailable
//!   ([`finish_activation`]), so the value is never null and a site is the
//!   slot load alone. A body without one keeps the per-site lookup.
//!
//! It is exact because a compiled activation runs start to finish on one
//! thread. Perry has no in-frame suspension: `await` and `yield` are lowered
//! by `perry-transform` (`async_to_generator`, `generator`) into state
//! machines whose every resume is a NEW call of the step function, which
//! computes its own address; an `await` that stays in a body
//! (`expr/fs_await.rs`) is a loop of runtime calls that return on the same
//! thread. A worker, a native callback or a closure called on another thread
//! is a new activation on that thread. A throw unwinds to a landing pad of the
//! same frame (`stmt/try_stmt.rs`), which the entry value dominates; wasm
//! (setjmp transport) takes the accessor call and hoists nothing.

use super::FnCtx;
use crate::types::PTR;

/// Slots in the runtime block. **Must equal `agent_ptrs::AGENT_PTR_SLOTS`.**
pub(crate) const AGENT_PTR_SLOTS: usize = crate::runtime_abi::AGENT_PTR_SLOTS;
/// `tls_hot::HOT_TLS_AGENT_PTRS_OFFSET` (pinned there by `offset_of!`).
const HOT_TLS_AGENT_PTRS_OFFSET: usize = crate::runtime_abi::HOT_TLS_AGENT_PTRS_OFFSET;
/// The block's exported symbol.
pub(crate) const AGENT_PTRS_SYMBOL: &str = "PERRY_AGENT_PTRS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentPtrAccess {
    ExecutableTls,
    WindowsTeb,
    AppleTsd,
    Call,
}

/// `PERRY_AGENT_PTRS`'s offset in the image's TLS block on Windows x86-64
/// (`agent_ptrs.rs`), and the image's TLS index.
pub(crate) const AGENT_PTRS_SECREL_SYMBOL: &str = "PERRY_AGENT_PTRS_SECREL";
pub(crate) const TLS_INDEX_SYMBOL: &str = "_tls_index";
/// `NT_TIB64`/`TEB64.ThreadLocalStoragePointer`, off `gs`.
const TEB_TLS_POINTER_OFFSET: &str = "88";

thread_local! {
    /// Whether the module being compiled is linked into an EXECUTABLE (set per
    /// module by `codegen::compile_module`); anything else must not assume
    /// a fixed thread-pointer offset (local-exec TLS).
    static OUTPUT_IS_EXECUTABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn set_output_is_executable(executable: bool) {
    OUTPUT_IS_EXECUTABLE.with(|c| c.set(executable));
}

/// Whether thread-locals of an `output_type` image for `triple` may be
/// addressed as fixed offsets from the thread pointer (local-exec): only in
/// an ELF executable, whose TLS block is the static one every thread is
/// created with. A `dlopen`ed image (a plugin, a shared library) must keep
/// the dynamic models. The compile applies it to its own module
/// (`LlModule::use_local_exec_tls`), so the object emitters, which run on
/// other threads, read the model from the IR rather than from shared state.
pub(crate) fn program_tls_is_local_exec(triple: &str, output_type: &str) -> bool {
    elf_executable_target(triple) && output_type == "executable"
}

/// An ELF target whose `executable` output is linked as an executable. An
/// Android or OpenHarmony program is a shared object the platform's app
/// process loads (`perry`'s `link/platform_cmd.rs` links it `-shared`)
/// whatever the output kind, so its thread-locals have no fixed offset from
/// the thread pointer, and local-exec relocations (`R_X86_64_TPOFF32`,
/// `R_AARCH64_TLSLE_*`) cannot be linked `-shared`. OpenHarmony's runtime
/// archives also use emulated TLS (`inprocess::optimize_emit::emulated_tls`),
/// whose blocks only the runtime's accessors reach.
fn elf_executable_target(triple: &str) -> bool {
    triple.contains("linux")
        && !triple.contains("android")
        && !triple.contains("ohos")
        && (triple.starts_with("x86_64") || triple.starts_with("aarch64"))
}

/// Android (API 29, `android_api_level!`): the program is a `dlopen`ed shared
/// object, so the block has no fixed offset from the thread pointer, but the
/// runtime's thread-locals are native ELF ones (`tls-model=global-dynamic`), so
/// the code names `PERRY_AGENT_PTRS` directly in the general-dynamic model
/// (`runtime_decls::objects`): one TLS-descriptor resolve per activation
/// ([`activation_slot_addr`]) instead of an accessor call (and, before API 29,
/// an emulated-TLS lookup inside it) per site. The slots are never null for
/// the same reason as on ELF executables (the block's static initializer).
fn android_native_tls(triple: &str) -> bool {
    triple.contains("android")
}

/// The image can address runtime TLS without a resolver call. Shared images
/// keep the generic accessor; each executable invocation stays on one thread.
pub(crate) fn executable_tls(triple: &str) -> bool {
    elf_executable_target(triple) && OUTPUT_IS_EXECUTABLE.with(|c| c.get())
}

/// How this compile reaches the block. `PERRY_AGENT_PTR_ACCESS=call` forces
/// the call (A/B knob).
pub(crate) fn agent_ptr_access(ctx: &FnCtx<'_>) -> AgentPtrAccess {
    if std::env::var("PERRY_AGENT_PTR_ACCESS").as_deref() == Ok("call") {
        return AgentPtrAccess::Call;
    }
    let triple = ctx.target_triple;
    if (elf_executable_target(triple) && OUTPUT_IS_EXECUTABLE.with(|c| c.get()))
        || android_native_tls(triple)
    {
        return AgentPtrAccess::ExecutableTls;
    }
    if triple.starts_with("x86_64") && triple.contains("windows") {
        return AgentPtrAccess::WindowsTeb;
    }
    if super::hot_tls::inline_hot_tls_enabled(ctx) {
        return AgentPtrAccess::AppleTsd;
    }
    AgentPtrAccess::Call
}

/// The current value of per-agent pointer `slot`, for a GC-leaf callee that
/// accepts `absent` (a constant operand meaning "not available here") in its
/// place: one thread-pointer-relative load in an ELF executable (the slot must never be
/// null there); the `HotTls` read on Apple aarch64, `absent` when the direct
/// TSD path is unavailable or the block is not published; the slot's `gc-leaf`
/// runtime accessor everywhere else. No null test and no fallback call on the
/// inline forms, so a site pays only the read. Ends in the block where the
/// returned register holds the value.
pub(crate) fn emit_agent_ptr_or(ctx: &mut FnCtx<'_>, slot: usize, absent: &str) -> String {
    debug_assert!(slot < AGENT_PTR_SLOTS);
    let slot_off = (slot * 8).to_string();
    let access = agent_ptr_access(ctx);
    if let Some(at) = activation_slot_addr(ctx, access, &slot_off) {
        return ctx.block().load(PTR, &at);
    }
    match access {
        AgentPtrAccess::ExecutableTls | AgentPtrAccess::WindowsTeb => {
            let at = emit_slot_addr(ctx, access, &slot_off);
            ctx.block().load(PTR, &at)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "agent_ptr");
            let field = super::hot_tls::hot_tls_field(
                ctx,
                &lookup.hot,
                &HOT_TLS_AGENT_PTRS_OFFSET.to_string(),
            );
            let blk = ctx.block();
            let block_ptr = blk.load(PTR, &field);
            let at = blk.gep(
                crate::types::I8,
                &block_ptr,
                &[(crate::types::I64, &slot_off)],
            );
            let val = blk.load(PTR, &at);
            let fast_pred = blk.label.clone();
            let join_idx = ctx.new_block("agent_ptr.join");
            let join_label = ctx.block_label(join_idx);
            ctx.block().br(&join_label);
            ctx.current_block = lookup.slow_idx;
            let slow_pred = ctx.block().label.clone();
            ctx.block().br(&join_label);
            ctx.current_block = join_idx;
            ctx.block()
                .phi(PTR, &[(&val, &fast_pred), (absent, &slow_pred)])
        }
        AgentPtrAccess::Call => ctx.block().call(PTR, agent_ptr_accessor(slot), &[]),
    }
}

/// May a compiled site use the agent slot of intrinsic `family` AS the
/// intrinsic object (dereference it beyond its layout word)? Only for a
/// family the realm's phase A binds (`perry_abi::realm_phase`): until phase B
/// has sealed a phase-B family or namespace its slot names the pristine
/// stand-in, which is good for the shadow word and for nothing else (DESIGN
/// startup-realm, section 7 rule 7).
pub(crate) fn intrinsic_slot_is_the_object(family: usize) -> bool {
    let name = match perry_abi::BuiltinProto::ALL.get(family) {
        Some((_, constructor)) => constructor,
        None => match perry_abi::BUILTIN_NAMESPACES.get(family - perry_abi::BUILTIN_PROTO_FAMILIES)
        {
            Some(namespace) => namespace,
            None => return false,
        },
    };
    !perry_abi::realm_rest_global(name)
}

/// [`emit_intrinsic_proto`] for a site that goes on to read the object
/// itself (its meta, its slots), not only its layout word: refused at
/// compile time for a phase-B family.
pub(crate) fn emit_intrinsic_proto_object(ctx: &mut FnCtx<'_>, family: usize) -> String {
    assert!(
        intrinsic_slot_is_the_object(family),
        "intrinsic family {family} is bound in phase B: its agent slot is the stand-in until then"
    );
    emit_intrinsic_proto(ctx, family)
}

/// The layout word of this agent's intrinsic prototype of builtin family
/// `family` (the word whose shadow bit is `Pristine(family)`): the one read
/// that is good for every family in every phase of the realm, the pristine
/// stand-in's word included.
pub(crate) fn emit_intrinsic_shadow_word(ctx: &mut FnCtx<'_>, family: usize) -> String {
    let proto = emit_intrinsic_proto(ctx, family);
    let at = ctx
        .block()
        .gep(crate::types::I8, &proto, &[(crate::types::I64, "4")]);
    ctx.block().load(crate::types::I32, &at)
}

/// This agent's intrinsic prototype of builtin family `family` (object model
/// step 3: the object whose layout word's shadow bit is `Pristine(family)`).
/// The slot is never null (`PERRY_PRISTINE_PROTO` until the realm is sealed),
/// so the inline forms are one load; Apple's TSD path answers the stand-in
/// when unavailable; every other target calls the leaf accessor.
///
/// Private: the two doors out of this module are
/// [`emit_intrinsic_shadow_word`] (the layout word, any family) and
/// [`emit_intrinsic_proto_object`] (the object, phase-A families only), so
/// the compiler enforces DESIGN startup-realm rule 7.
fn emit_intrinsic_proto(ctx: &mut FnCtx<'_>, family: usize) -> String {
    let slot = crate::runtime_abi::AGENT_PTR_INTRINSIC_PROTO + family;
    debug_assert!(slot < AGENT_PTR_SLOTS);
    let slot_off = (slot * 8).to_string();
    let access = agent_ptr_access(ctx);
    if let Some(at) = activation_slot_addr(ctx, access, &slot_off) {
        return ctx.block().load(PTR, &at);
    }
    match access {
        access @ (AgentPtrAccess::ExecutableTls | AgentPtrAccess::WindowsTeb) => {
            let at = emit_slot_addr(ctx, access, &slot_off);
            ctx.block().load(PTR, &at)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "intrinsic_proto");
            let field = super::hot_tls::hot_tls_field(
                ctx,
                &lookup.hot,
                &HOT_TLS_AGENT_PTRS_OFFSET.to_string(),
            );
            let blk = ctx.block();
            let block_ptr = blk.load(PTR, &field);
            let at = blk.gep(
                crate::types::I8,
                &block_ptr,
                &[(crate::types::I64, &slot_off)],
            );
            let val = blk.load(PTR, &at);
            let fast_pred = blk.label.clone();
            let join_idx = ctx.new_block("intrinsic_proto.join");
            let join_label = ctx.block_label(join_idx);
            ctx.block().br(&join_label);
            ctx.current_block = lookup.slow_idx;
            let called = ctx.block().call(
                PTR,
                "perry_intrinsic_proto",
                &[(crate::types::I32, &family.to_string())],
            );
            let slow_pred = ctx.block().label.clone();
            ctx.block().br(&join_label);
            ctx.current_block = join_idx;
            ctx.block()
                .phi(PTR, &[(&val, &fast_pred), (&called, &slow_pred)])
        }
        AgentPtrAccess::Call => ctx.block().call(
            PTR,
            "perry_intrinsic_proto",
            &[(crate::types::I32, &family.to_string())],
        ),
    }
}

/// The address of the slot `slot_off` bytes into this thread's block, for the
/// two forms that name the block through the thread pointer (module docs).
pub(crate) fn emit_slot_addr(
    ctx: &mut FnCtx<'_>,
    access: AgentPtrAccess,
    slot_off: &str,
) -> String {
    let blk = ctx.block();
    let block = match access {
        AgentPtrAccess::ExecutableTls => format!("@{AGENT_PTRS_SYMBOL}"),
        AgentPtrAccess::WindowsTeb => {
            // `mov gs:[0x58]` — a plain load in the x86 `gs` address space
            // (256). Plain, not volatile: it is re-read after every call, and
            // a thread switch happens only inside a call.
            let tls_array = blk.next_reg();
            blk.emit_raw(format!(
                "  {tls_array} = load ptr, ptr addrspace(256) inttoptr (i64 {TEB_TLS_POINTER_OFFSET} to ptr addrspace(256)), align 8"
            ));
            let index = blk.load(crate::types::I32, &format!("@{TLS_INDEX_SYMBOL}"));
            let index = blk.zext(crate::types::I32, &index, crate::types::I64);
            let entry = blk.gep(PTR, &tls_array, &[(crate::types::I64, &index)]);
            let image_block = blk.load(PTR, &entry);
            let secrel = blk.load(crate::types::I32, &format!("@{AGENT_PTRS_SECREL_SYMBOL}"));
            let secrel = blk.zext(crate::types::I32, &secrel, crate::types::I64);
            blk.gep(
                crate::types::I8,
                &image_block,
                &[(crate::types::I64, &secrel)],
            )
        }
        AgentPtrAccess::AppleTsd | AgentPtrAccess::Call => {
            unreachable!("{access:?} does not name the block through the thread pointer")
        }
    };
    blk.gep(crate::types::I8, &block, &[(crate::types::I64, slot_off)])
}

/// The address of the slot `slot_off` bytes into the block, from this
/// activation's one read of the block address (module docs), or `None` when
/// the site must read the address itself.
fn activation_slot_addr(
    ctx: &mut FnCtx<'_>,
    access: AgentPtrAccess,
    slot_off: &str,
) -> Option<String> {
    let block = match access {
        AgentPtrAccess::ExecutableTls => ctx.func.entry_tls_address(AGENT_PTRS_SYMBOL),
        AgentPtrAccess::AppleTsd => {
            let site = ctx.agent_block.as_mut()?;
            site.used = true;
            site.reg.clone()
        }
        AgentPtrAccess::WindowsTeb | AgentPtrAccess::Call => return None,
    };
    Some(
        ctx.block()
            .gep(crate::types::I8, &block, &[(crate::types::I64, slot_off)]),
    )
}

/// The start of a body whose activation reads the block address once
/// (Apple aarch64): an empty block every later block of the body is reached
/// through, and the register the read will define there.
pub(crate) struct ActivationBlock {
    head: String,
    body: String,
    reg: String,
    used: bool,
}

impl ActivationBlock {
    /// The edge [`finish_activation`] adds once the body is lowered: the
    /// start's head (still open) to its body.
    pub(crate) fn pending_edge(&self) -> (&str, &str) {
        (&self.head, &self.body)
    }
}

/// Open the body's start, at the point its statements begin. Sites lowered
/// from here on use [`ActivationBlock::reg`]; [`finish_activation`] emits
/// the read once the body is lowered, and only if a site used it.
pub(crate) fn begin_activation(ctx: &mut FnCtx<'_>) {
    if agent_ptr_access(ctx) != AgentPtrAccess::AppleTsd || ctx.block().is_terminated() {
        return;
    }
    let head = ctx.new_block("agent_block.head");
    let body = ctx.new_block("agent_block.body");
    let head_label = ctx.block_label(head);
    ctx.block().br(&head_label);
    ctx.current_block = body;
    let reg = ctx.block().next_reg();
    ctx.agent_block = Some(ActivationBlock {
        head: head_label,
        body: ctx.block_label(body),
        reg,
        used: false,
    });
}

/// Close the body's start: the TSD lookup of `HotTls::agent_ptrs`, or, when
/// the direct path is unavailable (no key, cache not published), the address
/// of the thread-local itself (`_tlv_get_addr`): both name this thread's
/// block, so the register is never null. An unused start is a branch.
pub(crate) fn finish_activation(ctx: &mut FnCtx<'_>) {
    let Some(site) = ctx.agent_block.take() else {
        return;
    };
    // Find the start by label: a pass may have inserted blocks before it.
    let index_of = |ctx: &FnCtx<'_>, label: &str| {
        ctx.func
            .blocks()
            .iter()
            .position(|b| b.label == label)
            .expect("the body start is a block of this function")
    };
    let resume = ctx.current_block;
    ctx.current_block = index_of(ctx, &site.head);
    let body_label = site.body;
    if site.used {
        let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "agent_block");
        let field =
            super::hot_tls::hot_tls_field(ctx, &lookup.hot, &HOT_TLS_AGENT_PTRS_OFFSET.to_string());
        let fast = ctx.block().load(PTR, &field);
        let fast_pred = ctx.block().label.clone();
        let join_idx = ctx.new_block("agent_block.join");
        let join_label = ctx.block_label(join_idx);
        ctx.block().br(&join_label);
        ctx.current_block = lookup.slow_idx;
        let slow = ctx.block().call(
            PTR,
            crate::function::TLS_ADDRESS_INTRINSIC,
            &[(PTR, &format!("@{AGENT_PTRS_SYMBOL}"))],
        );
        let slow_pred = ctx.block().label.clone();
        ctx.block().br(&join_label);
        ctx.current_block = join_idx;
        ctx.block().emit_raw(format!(
            "  {} = phi ptr [ {fast}, %{fast_pred} ], [ {slow}, %{slow_pred} ]",
            site.reg
        ));
    }
    ctx.block().br(&body_label);
    ctx.current_block = resume;
}

/// The `gc-leaf` runtime accessor of per-agent pointer `slot`.
fn agent_ptr_accessor(slot: usize) -> &'static str {
    match slot {
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR => "perry_shape_dir_cell",
        crate::runtime_abi::AGENT_PTR_CLASS_VALUES => "perry_class_value_dir_cell",
        _ => unreachable!("agent pointer slot {slot} has no accessor"),
    }
}

/// Emit a load of per-agent pointer `slot`, falling back to `fallback_fn`
/// (a `gc-leaf` runtime accessor `() -> ptr` that also publishes it). Ends
/// in a fresh block where the returned register holds the pointer.
pub(crate) fn emit_agent_ptr(ctx: &mut FnCtx<'_>, slot: usize, fallback_fn: &str) -> String {
    debug_assert!(slot < AGENT_PTR_SLOTS);
    let access = agent_ptr_access(ctx);
    if access == AgentPtrAccess::Call {
        return ctx.block().call(PTR, fallback_fn, &[]);
    }
    let slot_off = (slot * 8).to_string();
    let hoisted = activation_slot_addr(ctx, access, &slot_off);
    let (fast_pred, fast_val, slow_idx) = if hoisted.is_some()
        || matches!(
            access,
            AgentPtrAccess::ExecutableTls | AgentPtrAccess::WindowsTeb
        ) {
        let slow_idx = ctx.new_block("agent_ptr.slow");
        let fast_idx = ctx.new_block("agent_ptr.fast");
        let at = match hoisted {
            Some(at) => at,
            None => emit_slot_addr(ctx, access, &slot_off),
        };
        let blk = ctx.block();
        let val = blk.load(PTR, &at);
        let ok = blk.icmp_ne(PTR, &val, "null");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        ctx.block().cond_br(&ok, &fast_label, &slow_label);
        ctx.current_block = fast_idx;
        (ctx.block().label.clone(), val, slow_idx)
    } else {
        let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "agent_ptr");
        let fast_idx = ctx.new_block("agent_ptr.fast");
        let field =
            super::hot_tls::hot_tls_field(ctx, &lookup.hot, &HOT_TLS_AGENT_PTRS_OFFSET.to_string());
        let blk = ctx.block();
        let block_ptr = blk.load(PTR, &field);
        let at = blk.gep(
            crate::types::I8,
            &block_ptr,
            &[(crate::types::I64, &slot_off)],
        );
        let val = blk.load(PTR, &at);
        let ok = blk.icmp_ne(PTR, &val, "null");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(lookup.slow_idx);
        ctx.block().cond_br(&ok, &fast_label, &slow_label);
        ctx.current_block = fast_idx;
        (ctx.block().label.clone(), val, lookup.slow_idx)
    };
    let join_idx = ctx.new_block("agent_ptr.join");
    let join_label = ctx.block_label(join_idx);
    ctx.block().br(&join_label);
    ctx.current_block = slow_idx;
    let called = ctx.block().call(PTR, fallback_fn, &[]);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&join_label);
    ctx.current_block = join_idx;
    ctx.block()
        .phi(PTR, &[(&fast_val, &fast_pred), (&called, &slow_pred)])
}

#[cfg(test)]
mod tests {
    use super::*;
    /// DESIGN startup-realm, rule 7: before phase B a phase-B family's agent
    /// slot may be read for its layout word only. The raw slot loader is
    /// private to this module (the compiler enforces that); its two doors
    /// are the shadow-word reader (any family) and
    /// `emit_intrinsic_proto_object`, which refuses a phase-B family.
    #[test]
    fn a_phase_b_intrinsic_slot_is_never_used_as_the_object() {
        use perry_abi::{BuiltinProto, BUILTIN_PROTO_FAMILIES};
        // The families the generic dispatcher dereferences are phase A: this
        // fails the day one of them moves to phase B.
        assert!(intrinsic_slot_is_the_object(BuiltinProto::Map as usize));
        assert!(intrinsic_slot_is_the_object(BuiltinProto::Set as usize));
        assert!(intrinsic_slot_is_the_object(BuiltinProto::Array as usize));
        // Phase-B families and namespaces with a slot, by name (stage 2's
        // set; a later stage only adds to it): refused.
        assert!(!intrinsic_slot_is_the_object(
            BuiltinProto::WeakMap as usize
        ));
        assert!(!intrinsic_slot_is_the_object(
            BuiltinProto::WeakSet as usize
        ));
        for namespace in ["Intl", "Atomics"] {
            let family = perry_abi::builtin_namespace_family(namespace).expect(namespace);
            assert!(family >= BUILTIN_PROTO_FAMILIES);
            assert!(!intrinsic_slot_is_the_object(family), "{namespace}");
        }
        // No slot, no object.
        assert!(!intrinsic_slot_is_the_object(usize::MAX / 2));
        // Every phase-B name that has a slot is one of the refused: the
        // predicate follows the phase lists, whatever they hold.
        let mut refused = 0;
        for name in perry_abi::REALM_REST_GLOBALS {
            if let Some((family, _)) = BuiltinProto::ALL.iter().find(|(_, n)| n == name) {
                assert!(!intrinsic_slot_is_the_object(*family as usize), "{name}");
                refused += 1;
            }
            if let Some(family) = perry_abi::builtin_namespace_family(name) {
                assert!(!intrinsic_slot_is_the_object(family), "{name}");
                refused += 1;
            }
        }
        assert!(refused >= 4, "the phase-B list lost its slotted families");
    }

    /// DESIGN startup-realm, rule 7, on emitted code: a program that uses
    /// phase-B families reads their agent slots for the layout word (+4, an
    /// `i32`) and dereferences them nowhere else.
    #[test]
    fn generated_code_reads_only_the_layout_word_of_a_phase_b_slot() {
        let src = r#"
const k = {};
const wm = new WeakMap<object, number>();
wm.set(k, 1);
const ws = new WeakSet<object>();
ws.add(k);
console.log(wm.get(k), wm.has(k), ws.has(k));
const any: any = new Map<string, number>();
console.log(any.size);
"#;
        let ast = perry_parser::parse_typescript(src, "main.ts").unwrap();
        let module = perry_hir::lower_module(&ast, "main.ts", "main.ts").unwrap();
        let mut opts = crate::expr::class_field_barrier_tests::ir_opts();
        // The accessor form: every slot read is one `perry_intrinsic_proto` call.
        // (OpenHarmony: a shared-object target that keeps the accessor; Android
        // names the block directly in the general-dynamic TLS model.)
        opts.target = Some("aarch64-unknown-linux-ohos".into());
        let ir = String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap();
        // The lines of `body` that use register `reg` (not the one defining it).
        let uses = |body: &str, reg: &str| -> Vec<String> {
            body.lines()
                .map(str::trim)
                .filter(|l| !l.starts_with(&format!("{reg} =")))
                .filter(|l| {
                    l.match_indices(reg).any(|(at, _)| {
                        !l[at + reg.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
                    })
                })
                .map(str::to_string)
                .collect()
        };
        let mut seen = std::collections::BTreeSet::new();
        for body in ir.split("\ndefine ") {
            for line in body.lines() {
                let Some((slot, family)) = line
                    .trim()
                    .strip_suffix(')')
                    .and_then(|l| l.split_once(" = call ptr @perry_intrinsic_proto(i32 "))
                else {
                    continue;
                };
                let family: usize = family.parse().unwrap();
                if intrinsic_slot_is_the_object(family) {
                    continue;
                }
                seen.insert(family);
                for site in uses(body, slot) {
                    let at = site
                        .strip_suffix(&format!(" = getelementptr i8, ptr {slot}, i64 4"))
                        .unwrap_or_else(|| {
                            panic!("family {family}'s slot used as the object: {site}")
                        });
                    for load in uses(body, at) {
                        assert!(
                            load.ends_with(&format!(" = load i32, ptr {at}")),
                            "family {family}'s slot read beyond its layout word: {load}"
                        );
                    }
                }
            }
        }
        let expected = [
            perry_abi::BuiltinProto::WeakMap as usize,
            perry_abi::BuiltinProto::WeakSet as usize,
        ];
        assert!(
            expected.iter().all(|f| seen.contains(f)),
            "no phase-B slot read: {seen:?}"
        );
    }

    /// Local-exec only where the program is an ELF executable: Linux
    /// `executable` outputs. Android and OpenHarmony programs are shared
    /// objects whatever the output kind (`link/platform_cmd.rs` links them
    /// `-shared`), where a local-exec relocation does not link.
    #[test]
    fn local_exec_tls_only_for_elf_executables() {
        for triple in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
            assert!(program_tls_is_local_exec(triple, "executable"), "{triple}");
            for kind in ["dylib", "staticlib"] {
                assert!(!program_tls_is_local_exec(triple, kind), "{triple} {kind}");
            }
        }
        for triple in [
            "aarch64-linux-android",
            "x86_64-linux-android",
            "aarch64-unknown-linux-ohos",
            "x86_64-unknown-linux-ohos",
            "arm64-apple-macosx",
            "x86_64-pc-windows-msvc",
        ] {
            assert!(!program_tls_is_local_exec(triple, "executable"), "{triple}");
        }
    }

    /// The agent block's direct thread-pointer access follows the same image
    /// kind: an Android or OpenHarmony compile reaches it through the
    /// accessor even when its output kind says `executable`.
    #[test]
    fn shared_object_targets_take_the_accessor() {
        set_output_is_executable(true);
        assert!(executable_tls("x86_64-unknown-linux-gnu"));
        assert!(executable_tls("aarch64-unknown-linux-gnu"));
        for triple in [
            "aarch64-linux-android",
            "x86_64-linux-android",
            "aarch64-unknown-linux-ohos",
            "x86_64-unknown-linux-ohos",
        ] {
            assert!(!executable_tls(triple), "{triple}");
        }
        set_output_is_executable(false);
        assert!(!executable_tls("x86_64-unknown-linux-gnu"));
        set_output_is_executable(true);
    }
}

pub(crate) fn output_is_executable() -> bool {
    OUTPUT_IS_EXECUTABLE.with(|s| s.get())
}
