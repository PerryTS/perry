//! Emitted half of the "builtin prototype unmodified" proof (#11915):
//! `Pristine(P)` of the typed-entry contracts (decision 129).
//!
//! Object model step 3 (`OBJECT_MODEL_2026-10-13.md` §3.6): the fact is the
//! shadow bit of each family's intrinsic prototype, bit 30 of its layout word
//! (header `+4`). The runtime sets it once family `k`'s intrinsic prototype —
//! or `%Object.prototype%`, which every family inherits — is written,
//! defined, deleted or relinked after it sealed the realm, and publishes each
//! agent's intrinsic of family `k` in its agent-pointer slot
//! `AGENT_PTR_INTRINSIC_PROTO + k` (never null). A site that has proven its
//! receiver's KIND proves the method it is about to call is still that
//! kind's builtin with
//!
//! ```llvm
//! %p = load ptr, ptr <slot>                       ; mov %fs:off, %rax
//! %w = load i32, ptr %p+4                         ; cmpl $0xB0000000, 4(%rax)
//! %patched = icmp uge i32 %w, UNSHADOWED_WORD_END
//! ```
//!
//! folded into the branch the site already has (the own-override test, the
//! runtime kind or tag test, the inline push/pop admission). The patched side
//! is the site's generic arm: ONE call into the shared runtime path
//! (`js_builtin_proto_generic_call*` / the universal dispatcher), which
//! performs the real `Get(recv, name)` and call. No per-site fallback body, no
//! per-site state, no name classification, and no second copy of the fact.

use crate::block::LlBlock;
use crate::types::{I1, I32, I64};

/// The runtime's `BuiltinProto` discriminants (the agent slot offset). A runtime
/// test pins the two lists together (`builtin_proto_tests.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
// The full mirror, including families no lowering names yet.
#[allow(dead_code)]
pub(crate) enum BuiltinProto {
    Object = 0,
    Function = 1,
    Array = 2,
    String = 3,
    Number = 4,
    Boolean = 5,
    Symbol = 6,
    BigInt = 7,
    Map = 8,
    Set = 9,
    Date = 10,
    RegExp = 11,
    Promise = 12,
    Error = 13,
    WeakMap = 14,
    WeakSet = 15,
}

/// `i1`: true when `family`'s intrinsic prototype (or `%Object.prototype%`)
/// was modified, i.e. the direct builtin call is NOT proven: the shadow bit of
/// the agent's intrinsic, two loads and a compare.
pub(crate) fn emit_patched_test(ctx: &mut crate::expr::FnCtx<'_>, family: BuiltinProto) -> String {
    let end = (crate::runtime_abi::UNSHADOWED_WORD_END as i32).to_string();
    let proto = crate::expr::agent_ptr::emit_intrinsic_proto(ctx, family as usize);
    let at = ctx.block().gep(crate::types::I8, &proto, &[(I64, "4")]);
    let own = ctx.block().load(I32, &at);
    let own = ctx.block().icmp_uge(I32, &own, &end);
    let patched = if family == BuiltinProto::Object {
        own
    } else {
        let object = crate::expr::agent_ptr::emit_intrinsic_proto(ctx, BuiltinProto::Object as usize);
        let at = ctx.block().gep(crate::types::I8, &object, &[(I64, "4")]);
        let parent = ctx.block().load(I32, &at);
        let parent = ctx.block().icmp_uge(I32, &parent, &end);
        ctx.block().or(I1, &own, &parent)
    };
    let blk = ctx.block();
    // Unlikely, and said so: a guard's generic arm must not cost the
    // function's hot loops their register assignment (the same reason
    // `array_push_own`'s exit carries the hint).
    blk.call(I1, "llvm.expect.i1", &[(I1, &patched), (I1, "false")])
}

/// [`emit_patched_test`] over several families (a borrowed call also reads
/// `Function.prototype.call`; `toLocaleString()` admits several primitives).
pub(crate) fn emit_patched_any(ctx: &mut crate::expr::FnCtx<'_>, families: &[BuiltinProto]) -> String {
    let mut acc: Option<String> = None;
    for family in families {
        let t = emit_patched_test(ctx, *family);
        acc = Some(match acc {
            Some(prev) => ctx.block().or(I1, &prev, &t),
            None => t,
        });
    }
    acc.unwrap_or_else(|| "false".to_string())
}

/// What a direct-call site has proven about its receiver — the input of the
/// ONE pristine-receiver guard ([`emit_pristine_receiver`]): its KIND (HIR
/// fold, type, NaN-box tag, header type byte), so its methods are `family`'s
/// builtins while `Pristine(family)` holds. The check is one load and one
/// loads and one compare: Leaf, NoCollect, NoThrow.
#[allow(dead_code)]
pub(crate) enum PristineReceiverFact {
    Intrinsic(BuiltinProto),
}

/// `i1`: true when the fact HOLDS (the direct body is admitted). Emits no
/// call besides the `llvm.expect` hint on the intrinsic test.
#[allow(dead_code)]
pub(crate) fn emit_pristine_receiver(
    ctx: &mut crate::expr::FnCtx<'_>,
    fact: &PristineReceiverFact,
) -> String {
    match fact {
        PristineReceiverFact::Intrinsic(family) => {
            let patched = emit_patched_test(ctx, *family);
            ctx.block().xor(I1, &patched, "true")
        }
    }
}

/// Contract mode (decision 129 §4.5): with `PERRY_CHECK_CONTRACTS` set at
/// compile time, every direct arm a `Pristine(family)` proof admitted first
/// calls `js_builtin_proto_contract_check(family)` (runtime, every build),
/// which aborts naming the family if the fact does not hold. Release code
/// emits nothing.
pub(crate) fn emit_contract_check(blk: &mut LlBlock, families: &[BuiltinProto]) {
    if contract_mode() {
        for family in families {
            blk.call_void(
                "js_builtin_proto_contract_check",
                &[(I64, &(*family as u8).to_string())],
            );
        }
    }
}

fn contract_mode() -> bool {
    static MODE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        std::env::var_os("PERRY_CHECK_CONTRACTS").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// The ONE call a guarded site's generic arm makes: the shared runtime
/// helper, with the arguments in registers when there are at most three
/// (`js_builtin_proto_generic_call{0..3}`), else in a buffer.
pub(crate) fn emit_generic_call(
    ctx: &mut crate::expr::FnCtx<'_>,
    read: GenericRead,
    recv: &str,
    method: &str,
    args: &[String],
) -> String {
    use crate::types::{DOUBLE, I32, PTR};
    let key_idx = ctx.strings.intern(method);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    if read == GenericRead::Receiver && args.len() <= 3 {
        let name = format!("js_builtin_proto_generic_call{}", args.len());
        let mut operands: Vec<(crate::types::LlvmType, &str)> =
            vec![(DOUBLE, recv), (I64, method_id.as_str())];
        operands.extend(args.iter().map(|a| (DOUBLE, a.as_str())));
        return ctx.block().call(DOUBLE, &name, &operands);
    }
    let (argv, argc) = crate::lower_call::emit_method_args_buffer(ctx, args);
    ctx.block().call(
        DOUBLE,
        "js_builtin_proto_generic_call",
        &[
            (I32, &(read as i32).to_string()),
            (DOUBLE, recv),
            (I64, &method_id),
            (PTR, &argv),
            (I64, &argc),
        ],
    )
}

/// The checked string entries (runtime `object/builtin_proto_entries.rs`):
/// method, fewest and most arguments. `js_string_m_<method><argc>(recv,
/// args…)` tests `Pristine(String)` itself and takes the ONE generic path on
/// a miss, so a site that calls one carries no test of its own and passes
/// only the operands it evaluated.
pub(crate) const STRING_ENTRIES: &[(&str, usize, usize)] = &[
    ("slice", 0, 2),
    ("substring", 0, 2),
    ("substr", 1, 2),
    ("indexOf", 0, 2),
    ("lastIndexOf", 0, 2),
    ("includes", 0, 2),
    ("startsWith", 0, 2),
    ("endsWith", 0, 2),
    ("charAt", 0, 1),
    ("charCodeAt", 0, 1),
    ("codePointAt", 0, 1),
    ("at", 0, 1),
    ("split", 0, 2),
    ("replace", 0, 2),
    ("replaceAll", 0, 2),
    ("match", 0, 1),
    ("matchAll", 0, 1),
    ("search", 0, 1),
    ("toLowerCase", 0, 0),
    ("toUpperCase", 0, 0),
    ("trim", 0, 0),
    ("trimStart", 0, 0),
    ("trimEnd", 0, 0),
    ("padStart", 1, 2),
    ("padEnd", 1, 2),
    ("repeat", 1, 1),
    ("localeCompare", 0, 1),
    ("normalize", 0, 1),
    ("concat", 0, 2),
];

/// The checked entry for `method` called with `argc` arguments, if any.
pub(crate) fn string_entry(method: &str, argc: usize) -> Option<String> {
    STRING_ENTRIES
        .iter()
        .find(|(m, lo, hi)| *m == method && (*lo..=*hi).contains(&argc))
        .map(|(m, _, _)| format!("js_string_m_{m}{argc}"))
}

/// The checked Array entries (`js_array_m_<method><argc>`), for the folded
/// `arr.<method>(…)` nodes: `Pristine(Array)` plus the receiver's own proof
/// (no own property of the name, no own `[[Prototype]]`), in the callee.
pub(crate) const ARRAY_ENTRIES: &[(&str, usize, usize)] = &[
    ("forEach", 1, 1),
    ("map", 1, 1),
    ("filter", 1, 1),
    ("flatMap", 1, 1),
    ("find", 1, 1),
    ("findLast", 1, 1),
    ("findIndex", 1, 1),
    ("findLastIndex", 1, 1),
    ("some", 1, 1),
    ("every", 1, 1),
    ("sort", 1, 1),
    ("reduce", 1, 2),
    ("reduceRight", 1, 2),
    ("indexOf", 1, 2),
    ("lastIndexOf", 1, 2),
    ("includes", 1, 2),
    ("at", 1, 1),
    ("join", 0, 1),
    ("flat", 0, 0),
    ("toReversed", 0, 0),
    ("toSorted", 0, 1),
    ("with", 2, 2),
    ("entries", 0, 0),
    ("keys", 0, 0),
    ("values", 0, 0),
    ("slice", 1, 2),
    ("shift", 0, 0),
    // `arr.map(f)` whose result is discarded (`stmt/unused_expr.rs`).
    ("mapDiscard", 1, 1),
];

/// The checked Array entry for `method` called with `argc` arguments.
pub(crate) fn array_entry(method: &str, argc: usize) -> Option<String> {
    ARRAY_ENTRIES
        .iter()
        .find(|(m, lo, hi)| *m == method && (*lo..=*hi).contains(&argc))
        .map(|(m, _, _)| format!("js_array_m_{m}{argc}"))
}

/// Declare every checked entry: `double (double recv, double × argc)`.
pub(crate) fn declare_entries(module: &mut crate::module::LlModule) {
    use crate::types::DOUBLE;
    for (prefix, table) in [
        ("js_string_m_", STRING_ENTRIES),
        ("js_string_b_", STRING_ENTRIES),
        ("js_array_m_", ARRAY_ENTRIES),
        ("js_array_b_", ARRAY_ENTRIES),
    ] {
        for (method, lo, hi) in table {
            for argc in *lo..=*hi {
                let params = vec![DOUBLE; argc + 1];
                module.declare_function(&format!("{prefix}{method}{argc}"), DOUBLE, &params);
            }
        }
    }
}

/// Where the generic arm reads the method (runtime `GENERIC_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GenericRead {
    /// `Get(recv, name)`: an ordinary method call.
    Receiver = 0,
    /// `Get(%Array.prototype%, name)`: `Array.prototype.m.call(recv, …)`.
    ArrayPrototype = 1,
}

/// The guarded direct builtin call in OPERAND form: the site has already
/// evaluated (and rooted) `recv` and `args`; `direct` emits today's direct
/// sequence on them and returns its boxed `double`.
///
/// ```text
///   %patched = PATCHED[family] != 0         ; cmpb $0, sym+K(%rip)
///   br %patched, generic, direct
/// direct:  <unchanged direct sequence>
/// generic: js_builtin_proto_generic_call(read, recv, name_id, argv, argc)
/// ```
///
/// The generic arm is ONE call into the shared runtime helper — no per-site
/// fallback body, and it never re-enters `direct`, so nothing is re-read
/// across its collection point.
pub(crate) fn emit_guarded_direct(
    ctx: &mut crate::expr::FnCtx<'_>,
    families: &[BuiltinProto],
    read: GenericRead,
    recv: &str,
    method: &str,
    args: &[String],
    direct: impl FnOnce(&mut crate::expr::FnCtx<'_>) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    use crate::types::DOUBLE;
    let direct_idx = ctx.new_block("pristine.direct");
    let generic_idx = ctx.new_block("pristine.generic");
    let merge_idx = ctx.new_block("pristine.merge");
    let direct_label = ctx.block_label(direct_idx);
    let generic_label = ctx.block_label(generic_idx);
    let merge_label = ctx.block_label(merge_idx);
    {
        let patched = emit_patched_any(ctx, families);
        ctx.block().cond_br(&patched, &generic_label, &direct_label);
    }

    ctx.current_block = generic_idx;
    let generic_value = emit_generic_call(ctx, read, recv, method, args);
    let generic_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = direct_idx;
    emit_contract_check(ctx.block(), families);
    let direct_value = direct(ctx)?;
    let direct_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (direct_value.as_str(), direct_end.as_str()),
            (generic_value.as_str(), generic_end.as_str()),
        ],
    ))
}

#[cfg(test)]
#[path = "builtin_proto_ir_tests.rs"]
mod ir_tests;
