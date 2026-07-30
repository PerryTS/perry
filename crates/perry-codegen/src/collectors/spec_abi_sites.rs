//! Representation-selection Phase 2 (RFC `docs/representation-selection-rfc.md`
//! §5.4): whole-module, ctx-free pre-pass that derives **call-site
//! representation judgments** for direct user-function calls, so codegen can
//! emit one bounded specialized entry per function and route statically-proven
//! sites to it with **no per-call guard**.
//!
//! Collectors run before any `FnCtx` exists (same two-phase constraint as
//! `integer_locals`), so every judgment here is purely syntactic and
//! module-global:
//!
//! - **`TaPtr { kind, const_len }`** — the arg is a local/module binding whose
//!   SINGLE, top-level `let`/`const` binding is `new Int32Array(x)` (or another
//!   numeric typed-array kind) in a **non-view construction form**: `x` absent,
//!   an integer-literal length, or a local proven to be a plain array (its own
//!   single binding is an array literal, never reassigned). The binding local is
//!   never reassigned anywhere in the module (`LocalSet`/`GlobalSet`/`Update`)
//!   and never referenced inside any closure body — so at every dominated call
//!   site the slot still holds that exact typed array, whose header address,
//!   element kind, and length are fixed for the program's lifetime (typed-array
//!   storage is non-movable: `gc/types.rs` marks `GC_TYPE_TYPED_ARRAY` /
//!   `GC_TYPE_BUFFER` `movable: false`, and a non-view typed array cannot be
//!   detached). `const_len` is `Some` when the length is an integer literal or
//!   the element count of an array-literal source whose uses provably cannot
//!   change its length.
//! - **`I32`** — integer literal in i32 range.
//! - **`F64`** — any other numeric literal (a JS number's NaN-box IS its
//!   double bits, so raw-f64 passing is bit-identical).
//! - anything else → **`Boxed`** (always sound; the public boxed entry remains
//!   the permanent ABI).
//!
//! **Dominance** is judged structurally: a binding only "proves" for call
//! sites that appear in a LATER top-level statement of the same body (walking
//! nested branches/loops but never descending into closure bodies, which
//! execute at unknown times). Codegen mirrors the same rule at lowering time
//! via `FnCtx::spec_ta_ready`, so a collector/lowering disagreement can only
//! lose the fast path, never route an unproven value.
//!
//! Under-approximation is free everywhere: an unjudged/unproven site simply
//! keeps today's boxed call path.

use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Module, Stmt};

/// Per-parameter representation for a specialized entry. `Boxed` slots keep
/// the NaN-boxed `double` ABI; the others receive raw machine values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecParamRep {
    /// NaN-boxed double — today's uniform ABI, always sound.
    Boxed,
    /// Raw signed i32. Caller proves the value by construction (literal).
    I32,
    /// Raw double (bit-identical to the boxed form for numbers).
    F64,
    /// Raw `TypedArrayHeader*` of a proven non-view typed array.
    TaPtr {
        /// `perry_hir::TYPED_ARRAY_KIND_*` element kind.
        kind: u8,
        /// Compile-time-constant element count, when proven.
        const_len: Option<i64>,
    },
}

impl SpecParamRep {
    /// Symbol-mangle label. `[a-z0-9]` only so the composed symbol survives
    /// `sanitize`/`sanitize_member` byte-identically.
    pub fn label(&self) -> String {
        match self {
            Self::Boxed => "b".to_string(),
            Self::I32 => "i32".to_string(),
            Self::F64 => "f64".to_string(),
            Self::TaPtr { kind, const_len } => match const_len {
                Some(len) => format!("ta{kind}x{len}"),
                None => format!("ta{kind}"),
            },
        }
    }
}

/// A module binding proven to permanently hold one specific non-view typed
/// array (see module docs for the exact proof obligations).
#[derive(Debug, Clone, Copy)]
pub struct SpecTaBinding {
    pub kind: u8,
    pub const_len: Option<i64>,
}

/// Result of the pre-pass: proven typed-array bindings plus the per-callee
/// judged direct call sites.
#[derive(Debug, Default)]
pub struct SpecAbiModuleFacts {
    /// LocalId → proof for bindings usable as `TaPtr` args.
    pub ta_bindings: HashMap<u32, SpecTaBinding>,
    /// FuncId → one judged rep vector per direct `FuncRef` call site found in
    /// the module init or a function body (closure bodies excluded).
    pub call_sites: HashMap<u32, Vec<Vec<SpecParamRep>>>,
}

/// Numeric (non-BigInt, non-Float16) typed-array kinds — the set with a
/// `BufferElem` representation in codegen.
pub(crate) fn spec_ta_kind_is_numeric(kind: u8) -> bool {
    use perry_hir::*;
    matches!(
        kind,
        TYPED_ARRAY_KIND_INT8
            | TYPED_ARRAY_KIND_UINT8
            | TYPED_ARRAY_KIND_UINT8_CLAMPED
            | TYPED_ARRAY_KIND_INT16
            | TYPED_ARRAY_KIND_UINT16
            | TYPED_ARRAY_KIND_INT32
            | TYPED_ARRAY_KIND_UINT32
            | TYPED_ARRAY_KIND_FLOAT32
            | TYPED_ARRAY_KIND_FLOAT64
    )
}

/// Every local reassigned (`LocalSet`/`GlobalSet`/`Update`) anywhere in
/// `stmts`, including inside nested closure bodies. Element writes
/// (`x[i] = v`) and method calls are NOT reassignments — they cannot change
/// which typed array the binding refers to, its kind, or its length.
/// Scan once and probe the set when checking several ids against one body.
pub(crate) fn reassigned_locals(stmts: &[Stmt]) -> HashSet<u32> {
    let mut scan = ModuleScan::default();
    walk_stmts(stmts, 0, &mut scan);
    scan.writes
}

/// Every local reassigned in any executable body in `hir`.
///
/// Closure codegen seeds receiver types from module-wide declarations, so it
/// must pair that oracle with the equally broad reassignment set. Otherwise a
/// closure can specialize a captured receiver from its declared type even
/// after an enclosing body has replaced the binding with another value.
pub(crate) fn reassigned_locals_in_module(hir: &Module) -> HashSet<u32> {
    scan_whole_module(hir).writes
}

/// Single-id convenience over [`reassigned_locals`].
#[cfg(test)]
pub(crate) fn local_is_reassigned(stmts: &[Stmt], id: u32) -> bool {
    reassigned_locals(stmts).contains(&id)
}

/// Module-wide structural facts gathered in one walk.
#[derive(Default)]
struct ModuleScan {
    /// Reassignment targets (`LocalSet` / `GlobalSet` / `Update`), any depth.
    writes: HashSet<u32>,
    /// Locals referenced (read or written) inside any closure body, plus every
    /// closure capture list entry.
    closure_refs: HashSet<u32>,
    /// `Stmt::Let` occurrence count per id (a sound binding must be unique).
    let_counts: HashMap<u32, u32>,
    /// Uses of a local in any position other than `x[i]` read/write receiver
    /// or `new TypedArray(x)` constructor-argument. Such a use can't reassign
    /// the binding, but CAN change an array's length (`x.push(...)`,
    /// `g(x)`, `x.length = n`), so it only demotes `const_len`, not the
    /// binding proof itself.
    len_unsafe_uses: HashSet<u32>,
    /// Ids listed in `PreallocateBoxes` / `PreallocateTdzBoxes`: the slot (or
    /// module global) holds a BOX pointer, not the value — masking such a slot
    /// value would yield the box address, so these can never be `TaPtr`
    /// bindings.
    boxed_prealloc: HashSet<u32>,
}

fn record_expr_use(e: &Expr, depth: u32, scan: &mut ModuleScan) {
    match e {
        Expr::LocalGet(id) => {
            scan.len_unsafe_uses.insert(*id);
            if depth > 0 {
                scan.closure_refs.insert(*id);
            }
        }
        Expr::LocalSet(id, value) | Expr::GlobalSet(id, value) => {
            scan.writes.insert(*id);
            if depth > 0 {
                scan.closure_refs.insert(*id);
            }
            record_expr_use(value, depth, scan);
        }
        Expr::Update { id, .. } => {
            scan.writes.insert(*id);
            if depth > 0 {
                scan.closure_refs.insert(*id);
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| record_expr_use(c, depth, scan));
        }
        // Element-access receiver position is length-safe; everything nested
        // below it (index/value) is scanned normally.
        Expr::IndexGet { object, index } => {
            record_receiver_use(object, depth, scan);
            record_expr_use(index, depth, scan);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            record_receiver_use(object, depth, scan);
            record_expr_use(index, depth, scan);
            record_expr_use(value, depth, scan);
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            // Ordinary [[Set]] can invoke proxies/accessors and otherwise
            // expose either object, so neither position is length-safe.
            record_expr_use(target, depth, scan);
            record_expr_use(receiver, depth, scan);
            record_expr_use(key, depth, scan);
            record_expr_use(value, depth, scan);
        }
        // `new Int32Array(src)` consumes `src` by COPY (non-view: the arg is
        // not an ArrayBuffer when the binding proof later demands it), so the
        // ctor-arg position cannot change `src`'s length afterwards. Length
        // reads are also safe.
        Expr::TypedArrayNew { arg: Some(arg), .. } => {
            if let Expr::LocalGet(id) = arg.as_ref() {
                if depth > 0 {
                    scan.closure_refs.insert(*id);
                }
            } else {
                record_expr_use(arg, depth, scan);
            }
        }
        Expr::Closure {
            body,
            captures,
            params,
            ..
        } => {
            for c in captures {
                scan.closure_refs.insert(*c);
            }
            for p in params {
                if let Some(default) = &p.default {
                    record_expr_use(default, depth + 1, scan);
                }
            }
            walk_stmts(body, depth + 1, scan);
            // Defaults/keys hidden in the closure expr's other children.
            perry_hir::walker::walk_expr_children(e, &mut |c| {
                // The body statements were walked above; walk_expr_children
                // only yields child *expressions*, so this cannot re-enter the
                // body statement list.
                record_expr_use(c, depth + 1, scan)
            });
        }
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |c| record_expr_use(c, depth, scan));
        }
    }
}

/// Receiver position of an element access: a bare local read there is
/// length-safe (element loads/stores don't resize typed arrays and can't
/// reassign the binding). Anything more complex is scanned normally.
fn record_receiver_use(object: &Expr, depth: u32, scan: &mut ModuleScan) {
    if let Expr::LocalGet(id) = object {
        if depth > 0 {
            scan.closure_refs.insert(*id);
        }
    } else {
        record_expr_use(object, depth, scan);
    }
}

fn walk_stmts(stmts: &[Stmt], depth: u32, scan: &mut ModuleScan) {
    for s in stmts {
        walk_stmt(s, depth, scan);
    }
}

fn walk_stmt(s: &Stmt, depth: u32, scan: &mut ModuleScan) {
    match s {
        Stmt::Let { id, init, .. } => {
            *scan.let_counts.entry(*id).or_insert(0) += 1;
            if depth > 0 {
                scan.closure_refs.insert(*id);
            }
            if let Some(e) = init {
                record_expr_use(e, depth, scan);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => record_expr_use(e, depth, scan),
        Stmt::Return(Some(e)) => record_expr_use(e, depth, scan),
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
            scan.boxed_prealloc.extend(ids.iter().copied());
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            record_expr_use(condition, depth, scan);
            walk_stmts(then_branch, depth, scan);
            if let Some(eb) = else_branch {
                walk_stmts(eb, depth, scan);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            record_expr_use(condition, depth, scan);
            walk_stmts(body, depth, scan);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                walk_stmt(i, depth, scan);
            }
            if let Some(c) = condition {
                record_expr_use(c, depth, scan);
            }
            if let Some(u) = update {
                record_expr_use(u, depth, scan);
            }
            walk_stmts(body, depth, scan);
        }
        Stmt::Labeled { body, .. } => walk_stmt(body, depth, scan),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            walk_stmts(body, depth, scan);
            if let Some(c) = catch {
                walk_stmts(&c.body, depth, scan);
            }
            if let Some(f) = finally {
                walk_stmts(f, depth, scan);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            record_expr_use(discriminant, depth, scan);
            for case in cases {
                if let Some(t) = &case.test {
                    record_expr_use(t, depth, scan);
                }
                walk_stmts(&case.body, depth, scan);
            }
        }
    }
}

/// All executable bodies of the module: init, every function, every class
/// member body (constructors, methods, accessors, computed members, field
/// initializers). The write/closure scan must see all of them — a reassignment
/// hiding in a class method must disqualify a binding.
fn scan_whole_module(hir: &Module) -> ModuleScan {
    let mut scan = ModuleScan::default();
    walk_stmts(&hir.init, 0, &mut scan);
    for f in &hir.functions {
        walk_stmts(&f.body, 0, &mut scan);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            walk_stmts(&ctor.body, 0, &mut scan);
        }
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            walk_stmts(&m.body, 0, &mut scan);
        }
        for (_, g) in &c.getters {
            walk_stmts(&g.body, 0, &mut scan);
        }
        for (_, s) in &c.setters {
            walk_stmts(&s.body, 0, &mut scan);
        }
        for cm in &c.computed_members {
            record_expr_use(&cm.key_expr, 0, &mut scan);
            walk_stmts(&cm.function.body, 0, &mut scan);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(key) = &field.key_expr {
                record_expr_use(key, 0, &mut scan);
            }
            if let Some(init) = &field.init {
                record_expr_use(init, 0, &mut scan);
            }
        }
    }
    scan
}

/// Length of an array-literal expression when every element occupies exactly
/// one slot (no spread).
fn array_literal_len(e: &Expr) -> Option<i64> {
    match e {
        Expr::Array(elements) => Some(elements.len() as i64),
        Expr::ArraySpread(elements) => {
            if elements
                .iter()
                .any(|el| matches!(el, perry_hir::ArrayElement::Spread(_)))
            {
                None
            } else {
                Some(elements.len() as i64)
            }
        }
        _ => None,
    }
}

/// Judge the `new TypedArray(arg)` constructor argument: `Some(const_len)`
/// when the construction is provably NON-VIEW (arg is a length or a plain
/// array — never an ArrayBuffer), `None` when the form is unproven.
fn judge_ctor_arg(
    arg: Option<&Expr>,
    array_literal_locals: &HashMap<u32, Option<i64>>,
) -> Option<Option<i64>> {
    match arg {
        None => Some(Some(0)),
        Some(Expr::Integer(n)) if (0..=16_000_000).contains(n) => Some(Some(*n)),
        Some(Expr::Number(f))
            if f.is_finite() && f.fract() == 0.0 && *f >= 0.0 && *f <= 16_000_000.0 =>
        {
            Some(Some(*f as i64))
        }
        // A local whose single binding is an array literal, never reassigned:
        // definitely a plain array (copy construction, non-view). The length
        // is constant only when no use of the source could have resized it.
        Some(Expr::LocalGet(id)) => array_literal_locals.get(id).map(|len| *len),
        _ => None,
    }
}

/// One body's sequential site walk: top-level `Stmt::Let`s of proven bindings
/// make the binding "ready"; every direct `FuncRef` call anywhere below a
/// top-level statement is judged against the currently-ready set. Closure
/// bodies are never descended (they run at unknown times).
fn judge_sites_in_body(
    stmts: &[Stmt],
    ta_bindings: &HashMap<u32, SpecTaBinding>,
    out: &mut HashMap<u32, Vec<Vec<SpecParamRep>>>,
) {
    let mut ready: HashSet<u32> = HashSet::new();
    for s in stmts {
        judge_stmt(s, ta_bindings, &ready, out);
        if let Stmt::Let { id, .. } = s {
            if ta_bindings.contains_key(id) {
                ready.insert(*id);
            }
        }
    }
}

fn judge_stmt(
    s: &Stmt,
    ta: &HashMap<u32, SpecTaBinding>,
    ready: &HashSet<u32>,
    out: &mut HashMap<u32, Vec<Vec<SpecParamRep>>>,
) {
    match s {
        Stmt::Let { init: Some(e), .. } => judge_expr(e, ta, ready, out),
        Stmt::Let { init: None, .. } => {}
        Stmt::Expr(e) | Stmt::Throw(e) => judge_expr(e, ta, ready, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                judge_expr(e, ta, ready, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            judge_expr(condition, ta, ready, out);
            for st in then_branch {
                judge_stmt(st, ta, ready, out);
            }
            if let Some(eb) = else_branch {
                for st in eb {
                    judge_stmt(st, ta, ready, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            judge_expr(condition, ta, ready, out);
            for st in body {
                judge_stmt(st, ta, ready, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                judge_stmt(i, ta, ready, out);
            }
            if let Some(c) = condition {
                judge_expr(c, ta, ready, out);
            }
            if let Some(u) = update {
                judge_expr(u, ta, ready, out);
            }
            for st in body {
                judge_stmt(st, ta, ready, out);
            }
        }
        Stmt::Labeled { body, .. } => judge_stmt(body, ta, ready, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body {
                judge_stmt(st, ta, ready, out);
            }
            if let Some(c) = catch {
                for st in &c.body {
                    judge_stmt(st, ta, ready, out);
                }
            }
            if let Some(f) = finally {
                for st in f {
                    judge_stmt(st, ta, ready, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            judge_expr(discriminant, ta, ready, out);
            for case in cases {
                if let Some(t) = &case.test {
                    judge_expr(t, ta, ready, out);
                }
                for st in &case.body {
                    judge_stmt(st, ta, ready, out);
                }
            }
        }
        _ => {}
    }
}

/// Judge one argument expression. This is the collector twin of the
/// lowering-time per-slot match in `lower_call/func_ref.rs` — both must accept
/// the same shapes (the collector chooses the tuple; the lowering-time check
/// is the one that is load-bearing for soundness).
pub(crate) fn judge_arg(
    arg: &Expr,
    ta: &HashMap<u32, SpecTaBinding>,
    ready: &HashSet<u32>,
) -> SpecParamRep {
    match arg {
        Expr::Integer(n) if i32::try_from(*n).is_ok() => SpecParamRep::I32,
        Expr::Integer(_) => SpecParamRep::F64,
        Expr::Number(_) => SpecParamRep::F64,
        Expr::LocalGet(id) => match ta.get(id) {
            Some(binding) if ready.contains(id) => SpecParamRep::TaPtr {
                kind: binding.kind,
                const_len: binding.const_len,
            },
            _ => SpecParamRep::Boxed,
        },
        _ => SpecParamRep::Boxed,
    }
}

fn judge_expr(
    e: &Expr,
    ta: &HashMap<u32, SpecTaBinding>,
    ready: &HashSet<u32>,
    out: &mut HashMap<u32, Vec<Vec<SpecParamRep>>>,
) {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::FuncRef(fid) = callee.as_ref() {
            let judged: Vec<SpecParamRep> = args.iter().map(|a| judge_arg(a, ta, ready)).collect();
            out.entry(*fid).or_default().push(judged);
        }
    }
    // Closures execute at unknown times relative to the bindings — never
    // judge call sites inside them (their sites keep the boxed path).
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| judge_expr(c, ta, ready, out));
}

/// Run the whole pre-pass on a module.
pub fn collect_spec_abi_facts(hir: &Module) -> SpecAbiModuleFacts {
    let scan = scan_whole_module(hir);

    // Array-literal-bound, never-reassigned, non-closure-referenced locals:
    // proven plain arrays. Value is `Some(len)` when additionally no use could
    // have changed the length before/after construction (conservatively: the
    // local has NO uses outside element-access-receiver / TA-ctor-arg
    // positions — `len_unsafe_uses` records everything else).
    let mut array_literal_locals: HashMap<u32, Option<i64>> = HashMap::new();
    let mut collect_array_literals = |stmts: &[Stmt]| {
        for s in stmts {
            if let Stmt::Let {
                id, init: Some(e), ..
            } = s
            {
                if scan.let_counts.get(id).copied() == Some(1)
                    && !scan.writes.contains(id)
                    && !scan.closure_refs.contains(id)
                {
                    if let Some(len) = array_literal_len(e) {
                        let exact = !scan.len_unsafe_uses.contains(id);
                        array_literal_locals.insert(*id, exact.then_some(len));
                    }
                }
            }
        }
    };
    collect_array_literals(&hir.init);
    for f in &hir.functions {
        collect_array_literals(&f.body);
    }

    // Proven typed-array bindings: single TOP-LEVEL `let`/`const` bound to a
    // provably-non-view `TypedArrayNew` of a numeric kind, never reassigned,
    // never referenced by a closure.
    let mut ta_bindings: HashMap<u32, SpecTaBinding> = HashMap::new();
    let mut collect_ta = |stmts: &[Stmt]| {
        for s in stmts {
            if let Stmt::Let {
                id,
                init: Some(Expr::TypedArrayNew { kind, arg }),
                ..
            } = s
            {
                if !spec_ta_kind_is_numeric(*kind)
                    || scan.let_counts.get(id).copied() != Some(1)
                    || scan.writes.contains(id)
                    || scan.closure_refs.contains(id)
                    || scan.boxed_prealloc.contains(id)
                {
                    continue;
                }
                if let Some(const_len) = judge_ctor_arg(arg.as_deref(), &array_literal_locals) {
                    ta_bindings.insert(
                        *id,
                        SpecTaBinding {
                            kind: *kind,
                            const_len,
                        },
                    );
                }
            }
        }
    };
    collect_ta(&hir.init);
    for f in &hir.functions {
        collect_ta(&f.body);
    }

    // Judge every direct call site in init + function bodies.
    let mut call_sites: HashMap<u32, Vec<Vec<SpecParamRep>>> = HashMap::new();
    judge_sites_in_body(&hir.init, &ta_bindings, &mut call_sites);
    for f in &hir.functions {
        judge_sites_in_body(&f.body, &ta_bindings, &mut call_sites);
    }

    SpecAbiModuleFacts {
        ta_bindings,
        call_sites,
    }
}

#[cfg(test)]
mod tests;
