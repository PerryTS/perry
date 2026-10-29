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

pub(crate) use perry_abi::BuiltinProto;

/// `i1`: true when `family`'s intrinsic prototype (or `%Object.prototype%`)
/// was modified, i.e. the direct builtin call is NOT proven: the shadow bit of
/// the agent's intrinsic, two loads and a compare.
pub(crate) fn emit_patched_test(ctx: &mut crate::expr::FnCtx<'_>, family: BuiltinProto) -> String {
    let end = (crate::runtime_abi::LAYOUT_ORDINARY_END as i32).to_string();
    let proto = crate::expr::agent_ptr::emit_intrinsic_proto(ctx, family as usize);
    let at = ctx.block().gep(crate::types::I8, &proto, &[(I64, "4")]);
    let own = ctx.block().load(I32, &at);
    let own = ctx.block().icmp_uge(I32, &own, &end);
    let patched = if family == BuiltinProto::Object {
        own
    } else {
        let object =
            crate::expr::agent_ptr::emit_intrinsic_proto(ctx, BuiltinProto::Object as usize);
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
pub(crate) fn emit_patched_any(
    ctx: &mut crate::expr::FnCtx<'_>,
    families: &[BuiltinProto],
) -> String {
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

/// `i1`: true when builtin namespace `ns` is no longer proven pristine
/// (`Expr::NamespacePatched`, #12643): the shadow bit of the namespace's member
/// holder, which its agent slot names (runtime `object::builtin_namespace`).
/// One slot load, one word load and a compare; a name that is no builtin
/// namespace is never proven (`true`).
pub(crate) fn emit_namespace_patched(ctx: &mut crate::expr::FnCtx<'_>, ns: &str) -> String {
    let Some(family) = crate::runtime_abi::builtin_namespace_family(ns) else {
        return "true".to_string();
    };
    let end = (crate::runtime_abi::LAYOUT_ORDINARY_END as i32).to_string();
    let holder = crate::expr::agent_ptr::emit_intrinsic_proto(ctx, family);
    let at = ctx.block().gep(crate::types::I8, &holder, &[(I64, "4")]);
    let word = ctx.block().load(I32, &at);
    let patched = ctx.block().icmp_uge(I32, &word, &end);
    ctx.block()
        .call(I1, "llvm.expect.i1", &[(I1, &patched), (I1, "false")])
}

/// The member a guarded namespace call's generic arm reads
/// (`globalThis.<ns>.<member>(…)`, `patched_builtin_call`).
fn guarded_member(generic: &perry_hir::Expr) -> Option<&str> {
    use perry_hir::Expr;
    let callee = match generic {
        Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } => callee,
        _ => return None,
    };
    match callee.as_ref() {
        Expr::PropertyGet { property, .. } => Some(property.as_str()),
        _ => None,
    }
}

/// A guarded call's generic arm `globalThis.<ns>.<member>(args…)` without
/// spread: its family, member and arguments.
fn compact_generic<'e>(
    ns: &str,
    generic: &'e perry_hir::Expr,
) -> Option<(usize, &'e str, &'e [perry_hir::Expr])> {
    use perry_hir::Expr;
    let Expr::Call { callee, args, .. } = generic else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    let Expr::PropertyGet {
        object: global,
        property: name,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    if !matches!(global.as_ref(), Expr::GlobalThisExpr) || name != ns {
        return None;
    }
    let family = crate::runtime_abi::builtin_namespace_family(ns)?;
    Some((family, property.as_str(), args.as_slice()))
}

/// The generic arm as three out-of-line calls: read `globalThis[ns]` and its
/// `member` where the callee is read, evaluate the arguments, call the value
/// with the namespace as `this` (ECMA-262 `Call(GetValue(ref), base, args)`).
fn emit_namespace_generic_call(
    ctx: &mut crate::expr::FnCtx<'_>,
    family: usize,
    member: &str,
    args: &[perry_hir::Expr],
) -> anyhow::Result<String> {
    use crate::types::{DOUBLE, PTR};
    let ns = ctx.block().call(
        DOUBLE,
        "js_builtin_namespace_value",
        &[(I64, &family.to_string())],
    );
    let idx = ctx.strings.intern(member);
    let bytes = format!("@{}", ctx.strings.entry(idx).bytes_global);
    let len = member.len().to_string();
    let callee = ctx.block().call(
        DOUBLE,
        "js_builtin_namespace_member",
        &[(DOUBLE, &ns), (PTR, &bytes), (I64, &len)],
    );
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(crate::expr::lower_expr(ctx, arg)?);
    }
    let (argv, argc) = crate::lower_call::emit_method_args_buffer(ctx, &values);
    Ok(ctx.block().call(
        DOUBLE,
        "js_method_call_value",
        &[(DOUBLE, &callee), (DOUBLE, &ns), (PTR, &argv), (I64, &argc)],
    ))
}

/// A guarded namespace call (#12643):
/// `NamespacePatched(ns) ? <generic> : <intrinsic>`. The test runs where the
/// callee would be read, before either arm evaluates the arguments. Contract
/// mode first checks, in the intrinsic arm, that the clear bit is true
/// (`js_builtin_namespace_contract_check`).
pub(crate) fn lower_namespace_guard(
    ctx: &mut crate::expr::FnCtx<'_>,
    ns: &str,
    generic: &perry_hir::Expr,
    fast: &perry_hir::Expr,
) -> anyhow::Result<String> {
    use crate::types::{DOUBLE, PTR};
    let patched = emit_namespace_patched(ctx, ns);
    let generic_idx = ctx.new_block("ns_guard.generic");
    let fast_idx = ctx.new_block("ns_guard.intrinsic");
    let merge_idx = ctx.new_block("ns_guard.merge");
    let generic_label = ctx.block_label(generic_idx);
    let fast_label = ctx.block_label(fast_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&patched, &generic_label, &fast_label);

    ctx.current_block = generic_idx;
    let generic_val = match compact_generic(ns, generic) {
        Some((family, member, args)) => emit_namespace_generic_call(ctx, family, member, args)?,
        None => crate::expr::lower_expr(ctx, generic)?,
    };
    let generic_after = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fast_idx;
    if contract_mode() {
        if let (Some(family), Some(member)) = (
            crate::runtime_abi::builtin_namespace_family(ns),
            guarded_member(generic),
        ) {
            let idx = ctx.strings.intern(member);
            let ptr = format!("@{}", ctx.strings.entry(idx).bytes_global);
            let len = member.len().to_string();
            ctx.block().call_void(
                "js_builtin_namespace_contract_check",
                &[(I64, &family.to_string()), (PTR, &ptr), (I64, &len)],
            );
        }
    }
    let fast_val = crate::expr::lower_expr(ctx, fast)?;
    let fast_after = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&generic_val, &generic_after), (&fast_val, &fast_after)],
    ))
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

/// The builtin methods a method site on an unproven receiver calls directly
/// (runtime `object/builtin_method.rs`): `(name, argc, families)`, where
/// `families` is the set of builtin kinds whose intrinsic prototype holds
/// `name` as that builtin (1 String, 2 Array, 4 Map, 8 Set, 16 Function).
/// `js_bm_<name><argc>(site, recv, key, method_id, args…)` proves the
/// receiver's kind and `Pristine` of its family and calls the body; any other
/// receiver takes the universal dispatcher with the same operands.
pub(crate) const BUILTIN_METHODS: &[(&str, usize, u8)] = &[
    ("add", 1, 8),
    ("apply", 0, 16),
    ("apply", 1, 16),
    ("apply", 2, 16),
    ("at", 0, 1),
    ("at", 1, 3),
    ("call", 0, 16),
    ("call", 1, 16),
    ("call", 2, 16),
    ("call", 3, 16),
    ("call", 4, 16),
    ("charAt", 0, 1),
    ("charAt", 1, 1),
    ("charCodeAt", 0, 1),
    ("charCodeAt", 1, 1),
    ("codePointAt", 0, 1),
    ("codePointAt", 1, 1),
    ("concat", 0, 1),
    ("concat", 1, 1),
    ("concat", 2, 1),
    ("delete", 1, 12),
    ("endsWith", 0, 1),
    ("endsWith", 1, 1),
    ("endsWith", 2, 1),
    ("entries", 0, 2),
    ("every", 1, 2),
    ("filter", 1, 2),
    ("find", 1, 2),
    ("findIndex", 1, 2),
    ("findLast", 1, 2),
    ("findLastIndex", 1, 2),
    ("flat", 0, 2),
    ("flatMap", 1, 2),
    ("forEach", 1, 2),
    ("get", 1, 4),
    ("has", 1, 12),
    ("includes", 0, 1),
    ("includes", 1, 3),
    ("includes", 2, 3),
    ("indexOf", 0, 1),
    ("indexOf", 1, 3),
    ("indexOf", 2, 3),
    ("join", 0, 2),
    ("join", 1, 2),
    ("keys", 0, 2),
    ("lastIndexOf", 0, 1),
    ("lastIndexOf", 1, 3),
    ("lastIndexOf", 2, 3),
    ("localeCompare", 0, 1),
    ("localeCompare", 1, 1),
    ("map", 1, 2),
    ("match", 0, 1),
    ("match", 1, 1),
    ("matchAll", 0, 1),
    ("matchAll", 1, 1),
    ("normalize", 0, 1),
    ("normalize", 1, 1),
    ("padEnd", 1, 1),
    ("padEnd", 2, 1),
    ("padStart", 1, 1),
    ("padStart", 2, 1),
    ("pop", 0, 2),
    ("push", 1, 2),
    ("reduce", 1, 2),
    ("reduce", 2, 2),
    ("reduceRight", 1, 2),
    ("reduceRight", 2, 2),
    ("repeat", 1, 1),
    ("replace", 0, 1),
    ("replace", 1, 1),
    ("replace", 2, 1),
    ("replaceAll", 0, 1),
    ("replaceAll", 1, 1),
    ("replaceAll", 2, 1),
    ("search", 0, 1),
    ("search", 1, 1),
    ("set", 2, 4),
    ("shift", 0, 2),
    ("slice", 0, 1),
    ("slice", 1, 3),
    ("slice", 2, 3),
    ("some", 1, 2),
    ("sort", 1, 2),
    ("split", 0, 1),
    ("split", 1, 1),
    ("split", 2, 1),
    ("startsWith", 0, 1),
    ("startsWith", 1, 1),
    ("startsWith", 2, 1),
    ("substr", 1, 1),
    ("substr", 2, 1),
    ("substring", 0, 1),
    ("substring", 1, 1),
    ("substring", 2, 1),
    ("toLowerCase", 0, 1),
    ("toReversed", 0, 2),
    ("toSorted", 0, 2),
    ("toSorted", 1, 2),
    ("toUpperCase", 0, 1),
    ("trim", 0, 1),
    ("trimEnd", 0, 1),
    ("trimStart", 0, 1),
    ("values", 0, 2),
    ("with", 2, 2),
];

/// The direct builtin-method entry for `recv.<method>(argc args)`, if any.
pub(crate) fn builtin_method(method: &str, argc: usize) -> Option<String> {
    BUILTIN_METHODS
        .iter()
        .find(|(m, n, _)| *m == method && *n == argc)
        .map(|(m, n, _)| format!("js_bm_{m}{n}"))
}

/// A WeakMap/WeakSet method call on a receiver whose layout word is its
/// family's birth layout word (Z2, `runtime_abi::WEAKMAP_BIRTH_LAYOUT_WORD`): the
/// runtime operation it calls and, per admitting birth layout, the family
/// whose `Pristine` the call needs. `has`/`delete` serve both families.
pub(crate) fn weak_method_entry(
    method: &str,
    argc: usize,
) -> Option<(&'static str, &'static [(u32, BuiltinProto)])> {
    use crate::runtime_abi::{WEAKMAP_BIRTH_LAYOUT_WORD as WM, WEAKSET_BIRTH_LAYOUT_WORD as WS};
    const MAP: &[(u32, BuiltinProto)] = &[(WM, BuiltinProto::WeakMap)];
    const SET: &[(u32, BuiltinProto)] = &[(WS, BuiltinProto::WeakSet)];
    const BOTH: &[(u32, BuiltinProto)] =
        &[(WM, BuiltinProto::WeakMap), (WS, BuiltinProto::WeakSet)];
    Some(match (method, argc) {
        ("get", 1) => ("js_weakmap_get", MAP),
        ("set", 2) => ("js_weakmap_set", MAP),
        ("add", 1) => ("js_weakset_add", SET),
        ("has", 1) => ("js_weakmap_has", BOTH),
        ("delete", 1) => ("js_weakmap_delete", BOTH),
        _ => return None,
    })
}

/// Can a WeakMap or WeakSet answer `method` as a builtin (the Map and Set
/// families' bits of [`builtin_method_families`])?
pub(crate) fn weak_families(families: u8) -> bool {
    families & (4 | 8) != 0
}

/// The families holding `method` as a builtin at any argument count.
pub(crate) fn builtin_method_families(method: &str) -> u8 {
    BUILTIN_METHODS
        .iter()
        .filter(|(m, _, _)| *m == method)
        .fold(0, |acc, (_, _, f)| acc | f)
}

/// Declare every checked entry: `double (double recv, double × argc)`.
pub(crate) fn declare_entries(module: &mut crate::module::LlModule) {
    use crate::types::DOUBLE;
    for (method, argc, _) in BUILTIN_METHODS {
        let mut params = vec![
            crate::types::I64,
            DOUBLE,
            crate::types::I64,
            crate::types::I64,
        ];
        params.extend(std::iter::repeat_n(DOUBLE, *argc));
        module.declare_function(&format!("js_bm_{method}{argc}"), DOUBLE, &params);
    }
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

#[cfg(test)]
#[path = "untyped_receiver_dispatch_tests.rs"]
mod untyped_receiver_dispatch_tests;

const _: () = assert!(
    crate::runtime_abi::WEAKSET_BIRTH_LAYOUT_WORD
        == crate::runtime_abi::WEAKMAP_BIRTH_LAYOUT_WORD + 1,
    "method sites test both weak birth words with one range compare"
);
