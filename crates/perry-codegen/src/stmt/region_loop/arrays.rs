//! Step 4b regions, array slice S3: array facts inside a LOOP region.
//!
//! An array binding the loop cannot reassign, read as `xs[<index>]` where
//! the index has a static range `[0, c]` (`e & c`, or a literal), is a region
//! receiver of its own. The preheader checks, once:
//!
//! ```text
//! heap band -> guard word (ordinary array, not forwarded, no element
//! descriptors) -> prototype facts (protector byte + custom-proto bit)
//! -> c <u capacity
//! ```
//!
//! and derives the element base from the same header into a slot. An element
//! read in F-body is then `load base; load [base + 8 idx]; hole ? undefined`:
//! the bounds hold because `c < capacity`, a hole reads `undefined` because
//! the prototype facts hold, and a slot in `[length, capacity)` is a hole
//! (the S0 invariant). The read runs no JavaScript, so it is not a staleness
//! point for the region's other facts.
//!
//! # What keeps the facts true
//!
//! Anything that can run JavaScript can grow, reshape or re-prototype the
//! array: the planner stales every fact there, exactly as for object
//! receivers, and [`verify`](super::verify) checks it on the emitted IR.
//!
//! # GC
//!
//! The base is an address, and a collection can move the array. The base
//! slot is re-derived from the binding's root on the loop's poll arm
//! ([`emit_poll_refresh`]), at every re-check and in the preheader; nothing
//! else inside F-body may collect before a read ([`verify_arrays`]), and an
//! F-body path that collects must leave through a re-check (the dirty flag or
//! an every-iteration re-check), so the next iteration never reads through a
//! stale base.
//!
//! # Counter-indexed accesses and stores (#10741)
//!
//! An index is also proven when it is the loop's COUNTER (`for (...; i < B;
//! i++)`, see [`Env`]), or a body-local copy of it (`a[i] += v` spills its
//! base and key into `const` temps), so a real loop body over `a[i]` gets
//! the region too. Such a receiver, and any receiver the region STORES
//! into, is guarded as a DENSE raw-f64 array (`emit_array_region_guard`'s
//! `dense` facts): a bare read is one `load double` that is a Number by
//! construction ([`is_f64_index_read`]), and a store of a proven double is
//! one `store double` ([`try_lower_bare_index_set`]) that changes neither
//! the length, nor the layout, nor anything the collector traces.

use super::*;
use crate::inst::LlInst;

/// A static index bound above this is not a region fact (the guard compares
/// it against an `i32` capacity).
const MAX_STATIC_INDEX: i64 = 1 << 24;

/// One array receiver of a loop region.
#[derive(Clone)]
pub(crate) struct ArrayRecv {
    pub(super) recv: Recv,
    /// Every planned static-index access lies in `[0, max_index]`.
    pub(super) max_index: u32,
    /// The receiver's accesses need the dense raw-f64 facts (see the module
    /// doc): it is stored into, or indexed by the loop counter.
    pub(super) dense: bool,
    /// The region stores into it.
    pub(super) store: bool,
    /// A dense receiver not statically a plain `Array` may also be an owning
    /// `Float64Array` (`emit_typed_f64_region_guard`): its reads then
    /// canonicalise a NaN.
    pub(super) typed: bool,
    /// Some access is indexed by this loop counter (its bound is guarded).
    pub(super) counter: Option<Counter>,
    /// Body-local `const` copies of the binding (see [`Env`]).
    pub(super) aliases: Vec<u32>,
    /// `i64` alloca: the element base, valid while the region's `valid` flag is.
    pub(super) base_slot: String,
}

impl ArrayRecv {
    fn is(&self, object: &Expr) -> bool {
        match object {
            Expr::LocalGet(id) => self.recv == Recv::Local(*id) || self.aliases.contains(id),
            _ => false,
        }
    }
}

/// What a candidate array's accesses need, over every access in the body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ArrayUse {
    /// The largest static index (`0` when there is none).
    pub(super) max_index: u32,
    /// Some access is indexed by the loop counter.
    pub(super) counter: bool,
    /// Some access stores.
    pub(super) store: bool,
    /// Element receivers: some access loads an element by the loop counter
    /// for its VALUE (`const o = xs[i]`), not as a Number operand. That needs
    /// no dense facts: the guard proves `bound <= capacity` (the S0 invariant
    /// fills `[length, capacity)` with holes, and a hole reads `undefined`),
    /// and the loaded value's own shape guard is the proof of what it is.
    pub(super) element: bool,
}

impl ArrayUse {
    /// The dense raw-f64 facts are required (module doc).
    pub(super) fn dense(&self) -> bool {
        self.counter || self.store
    }
}

/// `for (...; i < B; i++)`: the counter `i`, written by the update and
/// nowhere else (not in the body, not in the condition), and its bound `B`,
/// an integer literal or a plain local nothing in the loop writes. At the
/// top of every iteration `i < B` held and, `i` being an integer `>= 0` at
/// the guard and only ever incremented, `0 <= i <= B - 1`: the guard checks
/// the entry value and `B <= length` once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counter {
    pub(super) id: u32,
    pub(super) bound: Bound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    Lit(i64),
    Local(u32),
}

/// The loop facts an index proof may use: the counter, and the body-local
/// copies (`const t = x;`) of a binding the loop never writes, each declared
/// once at the body's top level, never written again, and used only by the
/// statements after its declaration — so every use sees the value of the
/// binding it copies (the compound-assignment spill `a[i] += v` makes two).
#[derive(Clone, Default)]
pub(crate) struct Env {
    pub(super) counter: Option<Counter>,
    pub(super) aliases: HashMap<u32, u32>,
}

impl Env {
    pub(super) fn resolve(&self, id: u32) -> u32 {
        self.aliases.get(&id).copied().unwrap_or(id)
    }

    /// The array receiver an access's `object` names.
    pub(super) fn array(&self, object: &Expr) -> Option<Recv> {
        match object {
            Expr::LocalGet(id) => Some(Recv::Local(self.resolve(*id))),
            _ => None,
        }
    }

    /// A proven index: `Some(Some(c))` static in `[0, c]`, `Some(None)` the
    /// counter.
    pub(super) fn index(&self, e: &Expr) -> Option<Option<u32>> {
        if let Some(c) = static_index_max(e) {
            return Some(Some(c));
        }
        match (self.counter, e) {
            (Some(c), Expr::LocalGet(id)) if self.resolve(*id) == c.id => Some(None),
            _ => None,
        }
    }
}

/// An element store: `IndexSet`, or the `o[k] = v` reference form
/// `PutValueSet` with a computed key and the target as its own receiver.
/// `(object, index, value)`.
pub(super) fn element_store(e: &Expr) -> Option<(&Expr, &Expr, &Expr)> {
    match e {
        Expr::IndexSet {
            object,
            index,
            value,
        } => Some((object, index, value)),
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } if !matches!(key.as_ref(), Expr::String(_))
            && matches!((target.as_ref(), receiver.as_ref()),
                (Expr::LocalGet(a), Expr::LocalGet(b)) if a == b) =>
        {
            Some((target, key, value))
        }
        _ => None,
    }
}

/// The binding is DECLARED a plain `Array` (a hint, not a fact): its region
/// then admits only a dense array at the guard, and its reads skip the NaN
/// canonicalisation a `Float64Array` slot needs. A typed array passed there
/// fails the guard and runs today's loop.
pub(super) fn declared_plain_array(ctx: &FnCtx<'_>, r: Recv) -> bool {
    use perry_hir::types::Type as HirType;
    let Recv::Local(id) = r else {
        return false;
    };
    if ctx.reassigned_locals.contains(&id) {
        return false;
    }
    match crate::type_analysis::static_type_of(ctx, &Expr::LocalGet(id)) {
        Some(HirType::Array(_)) | Some(HirType::Tuple(_)) => true,
        Some(HirType::Generic { ref base, .. }) => base == "Array",
        _ => false,
    }
}

/// The binding is DECLARED a typed array whose elements are not raw `f64`
/// slots: any kind but `Float64Array` (`Buffer` included). No guard of an
/// array region can pass for one (the dense guard needs an `Array`, the typed
/// guard an owning `Float64Array`), and its elements are Numbers, never
/// element receivers, so it is no candidate: its region would add only a
/// guard that fails on every entry and a second copy of the loop. A hint
/// like [`declared_plain_array`]: a binding whose declaration is wrong only
/// keeps today's loop.
fn declared_typed_array_without_f64_slots(ctx: &FnCtx<'_>, r: Recv) -> bool {
    use perry_hir::types::Type as HirType;
    let Recv::Local(id) = r else {
        return false;
    };
    if ctx.reassigned_locals.contains(&id) {
        return false;
    }
    let name = match crate::type_analysis::static_type_of(ctx, &Expr::LocalGet(id)) {
        Some(HirType::Named(name)) => name,
        Some(HirType::Generic { base, .. }) => base,
        _ => return false,
    };
    matches!(
        name.as_str(),
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "BigInt64Array"
            | "BigUint64Array"
            | "Buffer"
    )
}

/// A local whose value only this function's visible writes can change.
fn plain_local(ctx: &FnCtx<'_>, id: u32) -> bool {
    (ctx.locals.contains_key(&id) || ctx.local_slot_reps.contains_key(&id))
        && !ctx.boxed_vars.contains(&id)
        && !ctx.prealloc_boxes.contains(&id)
        && !ctx.tdz_boxes.contains(&id)
        && !ctx.module_globals.contains_key(&id)
        && !ctx.closure_captures.contains_key(&id)
}

fn mentions(e: &Expr, id: u32) -> bool {
    let hit = match e {
        Expr::LocalGet(x) | Expr::LocalSet(x, _) => *x == id,
        Expr::Update { id: x, .. } => *x == id,
        _ => false,
    };
    let mut found = hit;
    if !found {
        perry_hir::walker::walk_expr_children(e, &mut |c| found |= mentions(c, id));
    }
    found
}

fn stmt_mentions(s: &Stmt, id: u32) -> bool {
    perry_hir::walker::stmt_any_expr(s, &mut |e| mentions(e, id))
}

/// The [`Env`] of a loop.
pub(super) fn loop_env(
    ctx: &FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> Env {
    let mut ctl: Vec<&Expr> = Vec::new();
    ctl.extend(cond);
    ctl.extend(update);
    // EVERY write in the loop: body, condition and update.
    let written = assigned(body, &ctl);
    let counter = (|| {
        let id = match update? {
            Expr::Update {
                id,
                op: perry_hir::UpdateOp::Increment,
                ..
            } => *id,
            _ => return None,
        };
        let Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } = cond?
        else {
            return None;
        };
        if !matches!(left.as_ref(), Expr::LocalGet(x) if *x == id) || !plain_local(ctx, id) {
            return None;
        }
        // The update is the counter's only write.
        if assigned(body, &[cond?]).contains(&id) {
            return None;
        }
        let bound = match right.as_ref() {
            Expr::Integer(k) if (0..=i64::from(i32::MAX)).contains(k) => Bound::Lit(*k),
            Expr::Number(n) if n.fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(n) => {
                Bound::Lit(*n as i64)
            }
            Expr::LocalGet(b) if *b != id && !written.contains(b) && plain_local(ctx, *b) => {
                Bound::Local(*b)
            }
            _ => return None,
        };
        Some(Counter { id, bound })
    })();
    let mut aliases = HashMap::new();
    for (k, s) in body.iter().enumerate() {
        let Stmt::Let {
            id,
            init: Some(Expr::LocalGet(src)),
            ..
        } = s
        else {
            continue;
        };
        let src_ok = counter.is_some_and(|c| c.id == *src)
            || (!written.contains(src) && receiver_eligible(ctx, Recv::Local(*src)));
        let decls = body
            .iter()
            .filter(|x| matches!(x, Stmt::Let { id: y, .. } if y == id))
            .count();
        // Written only by its declaration (`assigned` counts the `Let`, so
        // look for any other write), used only after it.
        let rewritten = body.iter().enumerate().any(|(j, x)| {
            j != k
                && perry_hir::walker::stmt_any_expr(x, &mut |e| {
                    let mut w = false;
                    fn writes(e: &Expr, id: u32, w: &mut bool) {
                        match e {
                            Expr::LocalSet(x, _) | Expr::Update { id: x, .. } if *x == id => {
                                *w = true
                            }
                            _ => {}
                        }
                        perry_hir::walker::walk_expr_children(e, &mut |c| writes(c, id, w));
                    }
                    writes(e, *id, &mut w);
                    w
                })
        });
        let used_before =
            body[..k].iter().any(|x| stmt_mentions(x, *id)) || ctl.iter().any(|e| mentions(e, *id));
        if src_ok
            && decls == 1
            && !rewritten
            && !used_before
            && !ctx.boxed_vars.contains(id)
            && !ctx.closure_captures.contains_key(id)
        {
            aliases.insert(*id, *src);
        }
    }
    Env { counter, aliases }
}

/// `PERRY_REGION_ELEMENTS=0` (compile time): no element-receiver loads (A/B).
pub(super) fn element_loads_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REGION_ELEMENTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `[0, c]` when `e` is `x & c` / `c & x` (`c >= 0`, the result of ToInt32
/// masking) or the literal `c`.
pub(super) fn static_index_max(e: &Expr) -> Option<u32> {
    fn lit(e: &Expr) -> Option<i64> {
        match e {
            Expr::Integer(c) => Some(*c),
            Expr::Number(n) if n.fract() == 0.0 && n.abs() <= MAX_STATIC_INDEX as f64 => {
                Some(*n as i64)
            }
            _ => None,
        }
    }
    let c = match e {
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        } => lit(right).or_else(|| lit(left))?,
        _ => lit(e)?,
    };
    (0..=MAX_STATIC_INDEX).contains(&c).then_some(c as u32)
}

/// Loop control that cannot collect: numeric compares and arithmetic over
/// locals and literals. A collection there would move an array between the
/// poll (which refreshes the base) and F-body.
fn quiet(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let num = |x: &Expr| crate::type_analysis::is_numeric_expr(ctx, x);
    match e {
        Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        Expr::Compare { left, right, .. } | Expr::Binary { left, right, .. } => {
            num(left) && num(right) && quiet(ctx, left) && quiet(ctx, right)
        }
        Expr::Update { id, .. } => num(&Expr::LocalGet(*id)),
        Expr::LocalSet(_, v) => num(v) && quiet(ctx, v),
        _ => false,
    }
}

/// The array candidates of a loop region: eligible bindings the loop never
/// assigns and never reads by static key, with what their proven-index
/// accesses need.
pub(super) fn candidates(
    ctx: &FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
    env: &Env,
) -> HashMap<Recv, ArrayUse> {
    let mut out: HashMap<Recv, ArrayUse> = HashMap::new();
    if cond.is_some_and(|c| !quiet(ctx, c)) || update.is_some_and(|u| !quiet(ctx, u)) {
        return out;
    }
    // (binding, static max or counter, store, a Number operand)
    let mut uses: Vec<(u32, Option<u32>, bool, bool)> = Vec::new();
    let mut walk = |e: &Expr| -> bool {
        // `numeric`: `e` is an operand a Number consumer reads (arithmetic,
        // a relational compare, a `Math.*` argument, a stored element). A
        // counter-indexed read anywhere else (`const o = xs[i]`) is not what
        // the dense facts serve: it does not make its array a candidate.
        fn e_walk(
            e: &Expr,
            env: &Env,
            numeric: bool,
            out: &mut Vec<(u32, Option<u32>, bool, bool)>,
        ) {
            let access = match e {
                Expr::IndexGet { object, index } => Some((object.as_ref(), index.as_ref(), false)),
                _ => element_store(e).map(|(o, i, _)| (o, i, true)),
            };
            if let Some((object, index, store)) = access {
                if let (Some(Recv::Local(id)), Some(ix)) = (env.array(object), env.index(index)) {
                    // Recorded for every counter/static-indexed access: a
                    // counter-indexed load for its value (`const o = xs[i]`)
                    // is an ELEMENT receiver's load and asks for no dense
                    // facts (see `candidates`).
                    out.push((id, ix, store, numeric));
                }
            }
            let consumes = match e {
                Expr::Binary { .. } => true,
                Expr::Unary { op, .. } => !matches!(op, UnaryOp::Not),
                Expr::Compare { op, .. } => {
                    matches!(
                        op,
                        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                    )
                }
                _ => element_store(e).is_some() || super::plan::pure_math_args(e).is_some(),
            };
            perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, env, consumes, out));
        }
        e_walk(e, env, false, &mut uses);
        false
    };
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut walk);
    }
    if uses.is_empty() {
        return out;
    }
    let mut extra: Vec<&Expr> = Vec::new();
    extra.extend(cond);
    extra.extend(update);
    let written = assigned(body, &extra);
    let keyed: HashSet<Recv> = accesses(body)
        .into_iter()
        .map(|(r, _, _, _, _)| match r {
            Recv::Local(id) => Recv::Local(env.resolve(id)),
            r => r,
        })
        .collect();
    // What a region serves an array: a store, a read a Number consumer takes,
    // or, in a body that runs no JS, any read at a proven index (one load in
    // place of the guarded tier, on facts that stay valid across iterations).
    // A body that calls out sets the dirty flag and re-checks the guard every
    // iteration, which an array only read for its element VALUES at a STATIC
    // index (`const o = xs[i & 63]; o.m()`) does not pay back: such an array
    // is no candidate there, though its reads ride along once some other
    // access makes it one. A counter-indexed element load (`const o = xs[i]`)
    // is the element-receiver case and is served wherever it appears.
    let calls = body
        .iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut may_call));
    let served: HashSet<u32> = uses
        .iter()
        .filter(|&&(_, ix, store, numeric)| store || numeric || !calls || ix.is_none())
        .map(|&(id, ..)| id)
        .collect();
    for (id, ix, store, numeric) in uses {
        let r = Recv::Local(id);
        // `dense`: the access asks for dense facts (not an element-only load).
        let dense = ix.is_some() || store || numeric;
        if !served.contains(&id)
            || written.contains(&id)
            || keyed.contains(&r)
            || !receiver_eligible(ctx, r)
            || declared_typed_array_without_f64_slots(ctx, r)
        {
            continue;
        }
        if !dense && !element_loads_enabled() {
            continue;
        }
        // An element-only use of a module-level binding is not admitted. The
        // region keeps the element base in a slot across the loop's poll and
        // re-derives it on the poll arm while the region is valid
        // (`emit_poll_refresh`). For a base derived from a module global,
        // gc-root-dominance's unrooted-alloca check treats the global's load
        // as a movable source and does not correlate that refresh with the
        // valid flag, so it reports the slot; the dense regions main already
        // forms over module globals trip it the same way outside its corpus.
        // Until the check can see the refresh, element regions stay off
        // module-level arrays.
        if !dense && ctx.module_globals.contains_key(&id) {
            continue;
        }
        let u = out.entry(r).or_default();
        match ix {
            Some(c) => u.max_index = u.max_index.max(c),
            None if dense => u.counter = true,
            None => u.element = true,
        }
        u.store |= store;
    }
    out
}

/// Does `e` contain a call (`f()`, `o.m()`, `new C()`, a native method call)?
fn may_call(e: &Expr) -> bool {
    if matches!(
        e,
        Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::NativeMethodCall { .. }
    ) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |c| found |= may_call(c));
    found
}

/// The preheader / re-check guard of one array receiver; stores the base.
pub(super) fn emit_guard(ctx: &mut FnCtx<'_>, a: &ArrayRecv) -> Result<String> {
    // The counter: an integer in `[0, i32::MAX]` here (it only grows), and
    // its bound as an `f64` for `bound <= length`.
    let mut counter_ok = "true".to_string();
    let mut bound = None;
    if let Some(c) = a.counter {
        let iv = lower_expr(ctx, &Expr::LocalGet(c.id))?;
        let b = match c.bound {
            Bound::Lit(k) => format!("{:?}", k as f64),
            Bound::Local(id) => lower_expr(ctx, &Expr::LocalGet(id))?,
        };
        let blk = ctx.block();
        let lo = blk.fcmp("oge", &iv, "0.0");
        let hi = blk.fcmp("ole", &iv, "2147483647.0");
        let in_range = blk.and(I1, &lo, &hi);
        // No `fptosi` of an out-of-range value (poison): convert a stand-in.
        let safe = blk.select(I1, &in_range, DOUBLE, &iv, "0.0");
        let as_int = blk.fptosi(DOUBLE, &safe, I32);
        let back = blk.sitofp(I32, &as_int, DOUBLE);
        let integral = blk.fcmp("oeq", &back, &iv);
        counter_ok = blk.and(I1, &in_range, &integral);
        bound = Some(b);
    }
    let recv_box = lower_recv(ctx, a.recv)?;
    let dense = a.dense.then_some(crate::expr::ArrayRegionDense {
        store: a.store,
        len_bound: bound.as_deref(),
    });
    // An element-only counter: `bound <= capacity` (see `ArrayUse::element`).
    let cap_bound = if a.dense { None } else { bound.as_deref() };
    let mut pass = crate::expr::emit_array_region_guard(
        ctx,
        &recv_box,
        a.max_index,
        dense,
        cap_bound,
        &a.base_slot,
    );
    if a.typed {
        // Not a dense array: an owning Float64Array serves the same raw slots.
        let ta = ctx.new_block("rloop.ta");
        let done = ctx.new_block("rloop.ta.done");
        let ta_l = ctx.block_label(ta);
        let done_l = ctx.block_label(done);
        let from = ctx.block().label.clone();
        ctx.block().cond_br(&pass, &done_l, &ta_l);
        ctx.current_block = ta;
        let pass_t = crate::expr::emit_typed_f64_region_guard(
            ctx,
            &recv_box,
            a.max_index,
            bound.as_deref(),
            &a.base_slot,
        );
        let ta_end = ctx.block().label.clone();
        ctx.block().br(&done_l);
        ctx.current_block = done;
        pass = ctx.block().phi(I1, &[("true", &from), (&pass_t, &ta_end)]);
    }
    Ok(ctx.block().and(I1, &pass, &counter_ok))
}

/// The `IndexGet` hook: a planned-bare element read in F-body.
pub(crate) fn try_lower_bare_index_get(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(a) = ctx.region_loop_facts.last() else {
        return Ok(None);
    };
    if a.arrays.is_empty() || !a.bare.contains(&(e as *const Expr as usize)) {
        return Ok(None);
    }
    let Expr::IndexGet { object, index } = e else {
        return Ok(None);
    };
    let Some(ar) = a.arrays.iter().find(|x| x.is(object)).cloned() else {
        return Ok(None);
    };
    // The planner proved `index` in range (static, or the guarded counter)
    // and JS-free.
    let idx = lower_expr(ctx, index)?;
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted_arr.push((b, i));
    }
    let blk = ctx.block();
    let idx = blk.fptosi(DOUBLE, &idx, I64);
    let base = blk.load(I64, &ar.base_slot);
    let off = blk.shl(I64, &idx, "3");
    let addr = blk.add(I64, &base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    let v = if ar.typed {
        // A Float64Array slot may hold any NaN payload.
        let raw = blk.load(DOUBLE, &ptr);
        let ordered = blk.fcmp("ord", &raw, &raw);
        blk.select(I1, &ordered, DOUBLE, &raw, "0x7FF8000000000000")
    } else if ar.dense {
        // Dense raw f64, in bounds: a canonical double, never a hole.
        blk.load(DOUBLE, &ptr)
    } else {
        let raw = blk.load(I64, &ptr);
        let hole = blk.icmp_eq(I64, &raw, crate::nanbox::TAG_HOLE_I64);
        let v = blk.select(I1, &hole, I64, crate::nanbox::TAG_UNDEFINED_I64, &raw);
        blk.bitcast_i64_to_double(&v)
    };
    stat(2, 1);
    Ok(Some(v))
}

/// The `IndexSet` hook: a planned-bare element store in F-body. The guard
/// proved a dense raw-f64 array with the index in `[0, length)` and the
/// integrity bits clear; a value [`expr_produces_canonical_raw_f64`] proves
/// a canonical double (asked again here, with the region's facts active)
/// then overwrites one raw slot: no length, layout or barrier work. A value
/// it cannot prove takes today's store (whose calls the verifier judges).
///
/// [`expr_produces_canonical_raw_f64`]: crate::type_analysis::expr_produces_canonical_raw_f64
pub(crate) fn try_lower_bare_index_set(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(a) = ctx.region_loop_facts.last() else {
        return Ok(None);
    };
    if a.arrays.is_empty() || !a.bare.contains(&(e as *const Expr as usize)) {
        return Ok(None);
    }
    let Some((object, index, value)) = element_store(e) else {
        return Ok(None);
    };
    let Some(ar) = a.arrays.iter().find(|x| x.is(object)).cloned() else {
        return Ok(None);
    };
    if !ar.dense || !ar.store || !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value)
    {
        return Ok(None);
    }
    let idx = lower_expr(ctx, index)?;
    let v = lower_expr(ctx, value)?;
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted_arr.push((b, i));
    }
    let blk = ctx.block();
    let idx = blk.fptosi(DOUBLE, &idx, I64);
    let base = blk.load(I64, &ar.base_slot);
    let off = blk.shl(I64, &idx, "3");
    let addr = blk.add(I64, &base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    // GC_STORE_AUDIT(POINTER_FREE): the region guard proved the array dense
    // raw-f64 and `expr_produces_canonical_raw_f64` the value a canonical
    // double — no GC pointer is written into the slot, so no write barrier.
    blk.store(DOUBLE, &v, &ptr);
    stat(3, 1);
    Ok(Some(v))
}

/// Is `e` a planned-bare element read of a DENSE region array (a canonical
/// double by the guard: the shared numeric-element predicate answers yes)?
pub(crate) fn is_f64_index_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::IndexGet { object, .. } = e else {
        return false;
    };
    ctx.region_loop_facts.last().is_some_and(|a| {
        a.bare.contains(&(e as *const Expr as usize))
            && a.arrays.iter().any(|x| x.dense && x.is(object))
    })
}

/// Is `e` a planned-bare element read of the active region? (The number
/// context's own tiers step aside for it.)
pub(crate) fn is_bare_index_get(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    ctx.region_loop_facts
        .last()
        .is_some_and(|a| !a.arrays.is_empty() && a.bare.contains(&(e as *const Expr as usize)))
}

/// The loop poll's arm: a collection may have moved every region array, so
/// each base whose region is valid is re-derived from the binding's root.
pub(crate) fn emit_poll_refresh(ctx: &mut FnCtx<'_>) -> Result<()> {
    let recipes: Vec<(String, ArrayRecv)> = ctx
        .region_loops
        .iter()
        .filter_map(|p| p.valid_slot.clone().map(|v| (v, p.arrays.clone())))
        .flat_map(|(v, arrs)| arrs.into_iter().map(move |a| (v.clone(), a)))
        .collect();
    for (valid_slot, a) in recipes {
        let go = ctx.new_block("rloop.arr.refresh");
        let done = ctx.new_block("rloop.arr.refreshed");
        let go_l = ctx.block_label(go);
        let done_l = ctx.block_label(done);
        let v = ctx.block().load(I1, &valid_slot);
        ctx.block().cond_br(&v, &go_l, &done_l);
        ctx.current_block = go;
        let recv_box = lower_recv(ctx, a.recv)?;
        let h = handle_of(ctx, &recv_box);
        let blk = ctx.block();
        let mut base = blk.array_elements_addr(&h);
        if a.typed {
            // A Float64Array (never moved) keeps its inline base.
            let ty_addr = blk.sub(I64, &h, "8");
            let ty_ptr = blk.inttoptr(I64, &ty_addr);
            let ty = blk.load(I8, &ty_ptr);
            let is_ta = blk.icmp_eq(I8, &ty, "11"); // GC_TYPE_TYPED_ARRAY
            let ta_base = blk.add(I64, &h, "16");
            base = blk.select(I1, &is_ta, I64, &ta_base, &base);
        }
        blk.store(I64, &base, &a.base_slot);
        blk.br(&done_l);
        ctx.current_block = done;
    }
    Ok(())
}

/// The array half of the F-body check, on the emitted IR: no instruction
/// that may collect lies on a path from the F entry to a bare element read,
/// and an F-body exit reached after one is covered by the next iteration's
/// re-check (every iteration, or the dirty flag set on that path).
pub(super) fn verify_arrays(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
    emitted: &[(usize, usize)],
    recheck: Recheck,
    dirty_slot: Option<&str>,
    valid_slot: Option<&str>,
) -> bool {
    if emitted.is_empty() {
        return true;
    }
    let blocks = ctx.func.blocks();
    let in_f = |b: usize| b == entry || (scan_start..scan_end).contains(&b);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for b in (scan_start..scan_end).chain(std::iter::once(entry)) {
        by_label.insert(blocks[b].label.as_str(), b);
    }
    let sets_dirty = |inst: &LlInst| match (inst, dirty_slot) {
        (LlInst::Store { val, ptr, .. }, Some(d)) => ptr == d && val == "true",
        _ => false,
    };
    // A nested body region's G-tail leaves the loop region: no later
    // iteration runs F-body on these facts.
    let leaves = |inst: &LlInst| match (inst, valid_slot) {
        (LlInst::Store { val, ptr, .. }, Some(v)) => ptr == v && val == "false",
        _ => false,
    };
    // State per block entry: the set of (collected, dirty-set, left)
    // combinations some path reaches it with, as an 8-bit mask over
    // `c | d << 1 | l << 2`.
    const C: u8 = 1;
    const D: u8 = 2;
    const L: u8 = 4;
    let diag = std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("4");
    let step = |mask: u8, inst: &LlInst| -> u8 {
        if diag && bare::inst_may_collect(inst) {
            let what = match inst {
                LlInst::Call { callee, .. } => callee.clone(),
                LlInst::Raw(s) => s.clone(),
                _ => "indirect".to_string(),
            };
            eprintln!("[perry region] array verify: may collect: {what}");
        }
        let mut out = 0u8;
        for combo in 0..8u8 {
            if mask & (1 << combo) == 0 {
                continue;
            }
            let mut c = combo;
            if bare::inst_may_collect(inst) {
                c |= C;
            }
            if sets_dirty(inst) {
                c |= D;
            }
            if leaves(inst) {
                c |= L;
            }
            out |= 1 << c;
        }
        out
    };
    // Any combination with C, for a read; for an exit, the combinations
    // with C that neither left the region nor (Dirty) set the flag.
    let collected = |mask: u8| (0..8u8).any(|c| c & C != 0 && mask & (1 << c) != 0);
    let collected_clean = |mask: u8| mask & (1 << C) != 0;
    let collected_staying =
        |mask: u8| (0..8u8).any(|c| c & C != 0 && c & L == 0 && mask & (1 << c) != 0);
    let mut state: HashMap<usize, u8> = HashMap::new();
    let mut work = vec![(entry, 1u8)];
    let mut exit_mask = 0u8;
    while let Some((b, m)) = work.pop() {
        let prev = state.get(&b).copied().unwrap_or(0);
        if prev | m == prev {
            continue;
        }
        let m = prev | m;
        state.insert(b, m);
        let mut cur = m;
        for inst in blocks[b].insts() {
            cur = step(cur, inst);
        }
        for succ in successors(&blocks[b]) {
            match by_label.get(succ.as_str()) {
                Some(&nb) if in_f(nb) => work.push((nb, cur)),
                _ => exit_mask |= cur,
            }
        }
    }
    for &(b, i) in emitted {
        let Some(&m) = state.get(&b) else { continue };
        let mut cur = m;
        for inst in &blocks[b].insts()[..i.min(blocks[b].insts().len())] {
            cur = step(cur, inst);
        }
        if collected(cur) {
            if diag {
                eprintln!("[perry region] array verify: read after a collection");
            }
            return false;
        }
    }
    if diag {
        eprintln!("[perry region] array verify: recheck={recheck:?} exit_mask={exit_mask:#x}");
    }
    match recheck {
        Recheck::Always => true,
        Recheck::Dirty => !collected_clean(exit_mask),
        Recheck::None => !collected_staying(exit_mask),
    }
}

/// A lowering that re-lowers a CLONE of planned HIR (a `let` redeclared
/// through `LocalSet`, #1803) keeps the plan: every clone node whose original
/// is a planned bare access or fact tree of the active region is planned too,
/// until [`unalias_clone`]. The verifier still judges the emitted IR.
pub(crate) fn alias_clone(ctx: &mut FnCtx<'_>, orig: &Expr, clone: &Expr) -> Vec<usize> {
    fn walk(orig: &Expr, clone: &Expr, a: &mut Active, out: &mut Vec<usize>) {
        let (o, c) = (orig as *const Expr as usize, clone as *const Expr as usize);
        if a.bare.contains(&o) && a.bare.insert(c) {
            out.push(c);
        }
        if a.trees.contains(&o) && a.trees.insert(c) {
            out.push(c);
        }
        let mut ok: Vec<&Expr> = Vec::new();
        let mut ck: Vec<&Expr> = Vec::new();
        perry_hir::walker::walk_expr_children(orig, &mut |x| ok.push(x));
        perry_hir::walker::walk_expr_children(clone, &mut |x| ck.push(x));
        if ok.len() == ck.len() {
            for (x, y) in ok.into_iter().zip(ck) {
                walk(x, y, a, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        walk(orig, clone, a, &mut out);
    }
    out
}

/// Drop the aliases [`alias_clone`] added (the clone is about to be freed,
/// and its addresses may be reused).
pub(crate) fn unalias_clone(ctx: &mut FnCtx<'_>, added: Vec<usize>) {
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        for p in added {
            a.bare.remove(&p);
            a.trees.remove(&p);
        }
    }
}
