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
//!
//! # Typed-array views (decision 69)
//!
//! A binding with a PROVEN inline-storage typed-array view (a spec clone's
//! `TaPtr` parameter, a fresh `new TA(n)` local: kind and data pointer fixed
//! for its lifetime, its length NOT — `buffer.transfer()` detaches it) is a
//! VIEW receiver. Its indices are proven by an interval evaluator over the
//! loop ([`Env::view_end`]): every index of a bare access lies in
//! `[0, end)`, `end` a constant or `B + c`, with `B` an invariant local or
//! a proven view's current length (optionally minus an invariant integer). The guard reads the length from the header with a plain load
//! and checks `end <= length`, once in the preheader and again at every
//! re-check; nothing else is assumed. A bare access is then today's proven
//! element access with its bounds proven (`BoundsProof::RegionGuard`): no
//! length load, no compare, no out-of-bounds arm, and a Number by
//! construction. Detaching or resizing runs JS, which stales the facts like
//! any other call, so the next use re-reads the length.

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
    /// A typed-array VIEW receiver (module doc): what its guard checks.
    pub(super) view: Option<ViewGuard>,
}

/// The guard of a view receiver: `end <= length` and `B + c <= length`.
#[derive(Clone)]
pub(crate) struct ViewGuard {
    /// The largest constant end of a bare access (`0`: none).
    pub(super) end: u32,
    /// The symbolic end `B + c` of the bare accesses indexed below `B`.
    pub(super) sym: Option<(Symbol, i64)>,
    /// The view's data-pointer slot and the header length's offset from it.
    pub(super) data_slot: String,
    pub(super) length_offset: i32,
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
    /// A typed-array VIEW receiver (module doc): its bare accesses' ends.
    pub(super) view: bool,
    /// View: the largest constant end (`hi + 1`) of a proven index.
    pub(super) view_end: u32,
    /// View: the symbolic end `B + c` (one loop-invariant local `B`).
    pub(super) view_sym: Option<(Symbol, i64)>,
    /// View: some proven index uses the loop counter's range, which holds
    /// only when the guard checked the counter's entry value.
    pub(super) view_counter: bool,
}

impl ArrayUse {
    /// The dense raw-f64 facts are required (module doc).
    pub(super) fn dense(&self) -> bool {
        !self.view && (self.counter || self.store)
    }

    /// View: does the guard of this use prove `end`?
    pub(super) fn view_covers(&self, end: ViewEnd) -> bool {
        self.view
            && match end.end {
                End::Const(e) => e <= i64::from(self.view_end),
                End::Sym(b, c) => self.view_sym.is_some_and(|(sb, sc)| sb == b && c <= sc),
            }
            && (!end.counter || self.view_counter)
    }
}

/// An integer range a view index proof may use: `lo <= v`, and `v <= c`
/// ([`Hi::Le`]) or `v < B + c` ([`Hi::Lt`], `B` a local the loop never
/// writes). Every value it describes is an exact integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rng {
    lo: i64,
    hi: Hi,
    /// Derived from the loop counter's range.
    counter: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hi {
    Le(i64),
    Lt(Symbol, i64),
}

/// Ranges stay far inside the exact-double integers.
const RANGE_LIMIT: i64 = 1 << 40;

impl Rng {
    fn exact(n: i64) -> Option<Rng> {
        (n.abs() <= RANGE_LIMIT).then_some(Rng {
            lo: n,
            hi: Hi::Le(n),
            counter: false,
        })
    }

    fn checked(lo: Option<i64>, hi: Option<Hi>, counter: bool) -> Option<Rng> {
        let lo = lo.filter(|v| v.abs() <= RANGE_LIMIT)?;
        let hi = match hi? {
            Hi::Le(c) if c.abs() <= RANGE_LIMIT && c >= lo => Hi::Le(c),
            Hi::Lt(b, c) if c.abs() <= RANGE_LIMIT => Hi::Lt(b, c),
            _ => return None,
        };
        Some(Rng { lo, hi, counter })
    }

    fn add(self, o: Rng) -> Option<Rng> {
        let hi = match (self.hi, o.hi) {
            (Hi::Le(a), Hi::Le(b)) => a.checked_add(b).map(Hi::Le),
            (Hi::Lt(s, a), Hi::Le(b)) | (Hi::Le(b), Hi::Lt(s, a)) => {
                a.checked_add(b).map(|c| Hi::Lt(s, c))
            }
            (Hi::Lt(..), Hi::Lt(..)) => None,
        };
        Rng::checked(self.lo.checked_add(o.lo), hi, self.counter || o.counter)
    }

    /// `self - k`, `k` an exact constant.
    fn sub_const(self, k: i64) -> Option<Rng> {
        let hi = match self.hi {
            Hi::Le(a) => a.checked_sub(k).map(Hi::Le),
            Hi::Lt(s, a) => a.checked_sub(k).map(|c| Hi::Lt(s, c)),
        };
        Rng::checked(self.lo.checked_sub(k), hi, self.counter)
    }

    /// Both non-negative with constant upper bounds.
    fn mul(self, o: Rng) -> Option<Rng> {
        match (self.hi, o.hi) {
            (Hi::Le(a), Hi::Le(b)) if self.lo >= 0 && o.lo >= 0 => Rng::checked(
                self.lo.checked_mul(o.lo),
                a.checked_mul(b).map(Hi::Le),
                self.counter || o.counter,
            ),
            _ => None,
        }
    }

    fn as_exact(self) -> Option<i64> {
        matches!(self.hi, Hi::Le(c) if c == self.lo).then_some(self.lo)
    }
}

/// The end of a proven view index: every value lies in `[0, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewEnd {
    pub(super) end: End,
    pub(super) counter: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// `v < c`.
    Const(i64),
    /// `v < B + c`.
    Sym(Symbol, i64),
}

/// The range of an index expression over `ranges`: integer literals, the
/// ranged locals, `+`, `-` a constant, `*` of non-negative bounded ranges,
/// `& mask` and `| 0`. Anything else has no range.
fn eval_range(e: &Expr, ranges: &HashMap<u32, Rng>) -> Option<Rng> {
    match e {
        Expr::Integer(n) => Rng::exact(*n),
        Expr::Number(n) if n.fract() == 0.0 && n.abs() <= RANGE_LIMIT as f64 => {
            Rng::exact(*n as i64)
        }
        Expr::LocalGet(id) => ranges.get(id).copied(),
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add => eval_range(left, ranges)?.add(eval_range(right, ranges)?),
            BinaryOp::Sub => {
                let k = eval_range(right, ranges)?.as_exact()?;
                eval_range(left, ranges)?.sub_const(k)
            }
            BinaryOp::Mul => eval_range(left, ranges)?.mul(eval_range(right, ranges)?),
            // `ToInt32(x) & m` with a literal `m >= 0` is in `[0, m]` for any `x`.
            BinaryOp::BitAnd => {
                let m = static_index_max(e)?;
                Rng::checked(Some(0), Some(Hi::Le(i64::from(m))), false)
            }
            // `x | 0` is `x` when `x` is an int32 already.
            BinaryOp::BitOr => {
                let (x, z) = match (left.as_ref(), right.as_ref()) {
                    (x, Expr::Integer(0)) | (Expr::Integer(0), x) => (x, ()),
                    _ => return None,
                };
                let _ = z;
                let r = eval_range(x, ranges)?;
                match r.hi {
                    Hi::Le(c) if r.lo >= i64::from(i32::MIN) && c <= i64::from(i32::MAX) => Some(r),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    }
}

/// `for (...; i < B; i++)`: the counter `i`, written by the update and
/// nowhere else (not in the body, not in the condition), and its bound `B`,
/// an integer literal, a plain local, or a proven view length. The loop
/// writes neither the bound binding nor an optional integer subtraction. At the
/// top of every iteration `i < B` held and, `i` being an integer `>= 0` at
/// the guard and only ever incremented, `0 <= i <= B - 1`: the guard checks
/// the entry value and `B <= length` once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counter {
    pub(super) id: u32,
    pub(super) bound: Bound,
}

/// The source of a symbolic end. A view length is a current header value,
/// never a construction length; JS stales the region before its next use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Symbol {
    Local(u32),
    ViewLength(u32, Option<u32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    Lit(i64),
    Local(u32),
    ViewLength(u32, Option<u32>, i64),
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
    /// View index proofs: the integer ranges of the loop counter, of nested
    /// counters and of body constants ([`view_ranges`]).
    pub(super) ranges: HashMap<u32, Rng>,
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

    /// A proven VIEW index: every value it takes in F-body lies in `[0, end)`.
    pub(super) fn view_end(&self, e: &Expr) -> Option<ViewEnd> {
        let r = eval_range(e, &self.ranges)?;
        if r.lo < 0 {
            return None;
        }
        let end = match r.hi {
            Hi::Le(c) => End::Const(c.checked_add(1).filter(|e| *e <= i64::from(i32::MAX))?),
            Hi::Lt(b, c) => End::Sym(b, c),
        };
        Some(ViewEnd {
            end,
            counter: r.counter,
        })
    }

    /// A proven index: `Some(Some(c))` static in `[0, c]`, `Some(None)` the
    /// counter.
    pub(super) fn index(&self, e: &Expr) -> Option<Option<u32>> {
        if let Some(c) = static_index_max(e) {
            return Some(Some(c));
        }
        match (self.counter, e) {
            (Some(c), Expr::LocalGet(id))
                if self.resolve(*id) == c.id && !matches!(c.bound, Bound::ViewLength(..)) =>
            {
                Some(None)
            }
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

/// A native header length, optional invariant integer subtraction and addend. This uses the same view
/// eligibility as element accesses; type annotations alone cannot prove it.
fn view_length_bound(ctx: &FnCtx<'_>, e: &Expr) -> Option<(u32, Option<u32>, i64)> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => {
            let Expr::LocalGet(id) = object.as_ref() else {
                return None;
            };
            view_of(ctx, *id)?;
            Some((*id, None, 0))
        }
        Expr::Binary {
            op: BinaryOp::Sub,
            left,
            right,
        } => {
            let (id, sub, c) = view_length_bound(ctx, left)?;
            if let Expr::LocalGet(k) = right.as_ref() {
                let r = crate::expr::int_range_expr(ctx, right)?;
                if sub.is_some()
                    || !plain_local(ctx, *k)
                    || r.min < -RANGE_LIMIT
                    || r.max > RANGE_LIMIT
                {
                    return None;
                }
                return Some((id, Some(*k), c));
            }
            let k = match right.as_ref() {
                Expr::Integer(k) => *k,
                Expr::Number(n) if n.fract() == 0.0 && n.abs() <= RANGE_LIMIT as f64 => *n as i64,
                _ => return None,
            };
            let c = c.checked_sub(k).filter(|v| v.abs() <= RANGE_LIMIT)?;
            Some((id, sub, c))
        }
        _ => None,
    }
}

/// A property read lowered as a direct native header load: no JavaScript.
pub(super) fn native_view_length(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(e, Expr::PropertyGet { .. }) && view_length_bound(ctx, e).is_some()
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
        let Expr::Compare { op, left, right } = cond? else {
            return None;
        };
        if !matches!(left.as_ref(), Expr::LocalGet(x) if *x == id) || !plain_local(ctx, id) {
            return None;
        }
        // The update is the counter's only write.
        if assigned(body, &[cond?]).contains(&id) {
            return None;
        }
        // Existing array counter proofs still require strict comparisons.
        // Only view-length bounds normalize <= to an exclusive end.
        let bound = match right.as_ref() {
            Expr::Integer(k) if *op == CompareOp::Lt && (0..=i64::from(i32::MAX)).contains(k) => {
                Bound::Lit(*k)
            }
            Expr::Number(n)
                if *op == CompareOp::Lt
                    && n.fract() == 0.0
                    && (0.0..=f64::from(i32::MAX)).contains(n) =>
            {
                Bound::Lit(*n as i64)
            }
            Expr::LocalGet(b)
                if *op == CompareOp::Lt
                    && *b != id
                    && !written.contains(b)
                    && plain_local(ctx, *b) =>
            {
                Bound::Local(*b)
            }
            e if matches!(op, CompareOp::Lt | CompareOp::Le) => {
                let (b, sub, c) = view_length_bound(ctx, e)?;
                if b == id
                    || written.contains(&b)
                    || !plain_local(ctx, b)
                    || sub.is_some_and(|k| k == id || written.contains(&k))
                {
                    return None;
                }
                let c = c.checked_add(i64::from(*op == CompareOp::Le))?;
                if c.abs() > RANGE_LIMIT {
                    return None;
                }
                Bound::ViewLength(b, sub, c)
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
    let ranges = view_ranges(ctx, counter, body, &written);
    Env {
        counter,
        aliases,
        ranges,
    }
}

/// Every local any statement of `ss` (nested too) declares, with its count.
fn declared(ss: &[Stmt], out: &mut HashMap<u32, usize>) {
    for s in ss {
        match s {
            Stmt::Let { id, .. } => *out.entry(*id).or_default() += 1,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                declared(then_branch, out);
                if let Some(e) = else_branch {
                    declared(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => declared(body, out),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    declared(std::slice::from_ref(i.as_ref()), out);
                }
                declared(body, out);
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    declared(&c.body, out);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                declared(body, out);
                if let Some(c) = catch {
                    declared(&c.body, out);
                }
                if let Some(f) = finally {
                    declared(f, out);
                }
            }
            _ => {}
        }
    }
}

/// Locals some expression of `ss` or `extra` WRITES (`x = v`, `x++`); a
/// declaration is not a write.
fn writes(ss: &[Stmt], extra: &[&Expr]) -> HashSet<u32> {
    fn e_walk(e: &Expr, out: &mut HashSet<u32>) {
        match e {
            Expr::LocalSet(x, _) | Expr::Update { id: x, .. } => {
                out.insert(*x);
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    let mut out = HashSet::new();
    for s in ss {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            e_walk(e, &mut out);
            false
        });
    }
    for e in extra {
        e_walk(e, &mut out);
    }
    out
}

/// The integer ranges a view index proof may use inside the loop body
/// (module doc):
/// - the loop counter: `[0, B - 1]`, or `< B` for a local `B` (the guard
///   checks the counter's entry value, see [`emit_guard`]);
/// - the counter of a nested `for (let j = INIT; j < B2; j++)`, declared by
///   its init and written only by its update, `INIT`'s range starting at
///   `>= 0`, `B2` a literal or a local the loop never writes: `[lo, B2)`;
/// - a body local declared once and never written: its initialiser's range.
///
/// A local's id is unique in its function and every use of it lies in its
/// scope, where the range holds.
fn view_ranges(
    ctx: &FnCtx<'_>,
    counter: Option<Counter>,
    body: &[Stmt],
    written: &HashSet<u32>,
) -> HashMap<u32, Rng> {
    let mut ranges = HashMap::new();
    if let Some(c) = counter {
        let hi = match c.bound {
            Bound::Lit(k) => Hi::Le(k - 1),
            Bound::Local(b) => Hi::Lt(Symbol::Local(b), 0),
            Bound::ViewLength(b, sub, c) => Hi::Lt(Symbol::ViewLength(b, sub), c),
        };
        if let Some(r) = Rng::checked(Some(0), Some(hi), true) {
            ranges.insert(c.id, r);
        }
    }
    // A local the loop never writes (nor declares) keeps the range the
    // compiler proves for it at the loop's entry (`int_range_expr`: integer
    // leaves only, `None` at the first unknown one).
    {
        let mut outer: Vec<u32> = Vec::new();
        for st in body {
            perry_hir::walker::stmt_any_expr(st, &mut |e| {
                fn locals(e: &Expr, out: &mut Vec<u32>) {
                    if let Expr::LocalGet(id) = e {
                        out.push(*id);
                    }
                    perry_hir::walker::walk_expr_children(e, &mut |c| locals(c, out));
                }
                locals(e, &mut outer);
                false
            });
        }
        for id in outer {
            if written.contains(&id) || ranges.contains_key(&id) || !plain_local(ctx, id) {
                continue;
            }
            if let Some(r) = crate::expr::int_range_expr(ctx, &Expr::LocalGet(id)) {
                if let Some(r) = Rng::checked(Some(r.min), Some(Hi::Le(r.max)), false) {
                    ranges.insert(id, r);
                }
            }
        }
    }
    let mut decls = HashMap::new();
    declared(body, &mut decls);
    let w = writes(body, &[]);
    let invariant = |b: u32| !written.contains(&b) && plain_local(ctx, b);
    let ok_local = |id: u32| {
        decls.get(&id) == Some(&1)
            && !ctx.boxed_vars.contains(&id)
            && !ctx.prealloc_boxes.contains(&id)
            && !ctx.tdz_boxes.contains(&id)
            && !ctx.closure_captures.contains_key(&id)
            && !ctx.module_globals.contains_key(&id)
    };
    fn walk(
        ss: &[Stmt],
        ranges: &mut HashMap<u32, Rng>,
        w: &HashSet<u32>,
        ok_local: &dyn Fn(u32) -> bool,
        invariant: &dyn Fn(u32) -> bool,
    ) {
        for s in ss {
            match s {
                Stmt::Let {
                    id, init: Some(e), ..
                } if ok_local(*id) && !w.contains(id) => {
                    if let Some(r) = eval_range(e, ranges) {
                        ranges.insert(*id, r);
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, ranges, w, ok_local, invariant);
                    if let Some(e) = else_branch {
                        walk(e, ranges, w, ok_local, invariant);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    walk(body, ranges, w, ok_local, invariant)
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    let counter = (|| {
                        let Some(Stmt::Let {
                            id, init: Some(ie), ..
                        }) = init.as_deref()
                        else {
                            return None;
                        };
                        let Some(Expr::Update {
                            id: u,
                            op: perry_hir::UpdateOp::Increment,
                            ..
                        }) = update
                        else {
                            return None;
                        };
                        let Some(Expr::Compare {
                            op: CompareOp::Lt,
                            left,
                            right,
                        }) = condition
                        else {
                            return None;
                        };
                        if u != id
                            || !matches!(left.as_ref(), Expr::LocalGet(x) if x == id)
                            || !ok_local(*id)
                            || writes(body, &[]).contains(id)
                        {
                            return None;
                        }
                        let lo = eval_range(ie, ranges)?;
                        let hi = match right.as_ref() {
                            Expr::Integer(k) if (0..=i64::from(i32::MAX)).contains(k) => {
                                Hi::Le(*k - 1)
                            }
                            Expr::Number(n)
                                if n.fract() == 0.0 && (0.0..=f64::from(i32::MAX)).contains(n) =>
                            {
                                Hi::Le(*n as i64 - 1)
                            }
                            Expr::LocalGet(b) if *b != *id && invariant(*b) => {
                                Hi::Lt(Symbol::Local(*b), 0)
                            }
                            _ => return None,
                        };
                        let r = match hi {
                            // An empty range: the body never runs.
                            Hi::Le(c) if c < lo.lo => return None,
                            hi => Rng::checked(Some(lo.lo), Some(hi), lo.counter)?,
                        };
                        (r.lo >= 0).then_some((*id, r))
                    })();
                    if let Some((id, r)) = counter {
                        ranges.insert(id, r);
                    }
                    walk(body, ranges, w, ok_local, invariant);
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(&c.body, ranges, w, ok_local, invariant);
                    }
                }
                _ => {}
            }
        }
    }
    walk(body, &mut ranges, &w, &ok_local, &invariant);
    ranges
}

/// The view of `id` when it can be a VIEW receiver: a proven inline-storage
/// typed-array view (`expr::proven_view_receiver`), element-indexed, with
/// its length in the header (no cached length slot), and a local of this
/// function.
pub(super) fn view_of(ctx: &FnCtx<'_>, id: u32) -> Option<crate::native_value::BufferViewSlot> {
    if !view_regions_enabled() || ctx.module_globals.contains_key(&id) {
        return None;
    }
    let (_, view) = crate::expr::proven_view_receiver(ctx, &Expr::LocalGet(id))?;
    (view.length_slot.is_none()
        && view.view_byte_offset.unwrap_or(0) == 0
        && view.element_width_bytes.is_power_of_two())
    .then_some(view)
}

/// `PERRY_REGION_VIEWS=0` (compile time): no view receivers (A/B).
pub(super) fn view_regions_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REGION_VIEWS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
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
    fn num(ctx: &FnCtx<'_>, e: &Expr) -> bool {
        match e {
            Expr::Binary { left, right, .. } => num(ctx, left) && num(ctx, right),
            _ => native_view_length(ctx, e) || crate::type_analysis::is_numeric_expr(ctx, e),
        }
    }
    match e {
        Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        Expr::Compare { left, right, .. } | Expr::Binary { left, right, .. } => {
            num(ctx, left) && num(ctx, right) && quiet(ctx, left) && quiet(ctx, right)
        }
        Expr::Update { id, .. } => num(ctx, &Expr::LocalGet(*id)),
        Expr::LocalSet(_, v) => num(ctx, v) && quiet(ctx, v),
        Expr::PropertyGet { .. } => native_view_length(ctx, e),
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
    let mut extra: Vec<&Expr> = Vec::new();
    extra.extend(cond);
    extra.extend(update);
    let written = assigned(body, &extra);
    out.extend(view_candidates(ctx, body, env, &written));
    if uses.is_empty() {
        return out;
    }
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

/// The VIEW receivers of a loop (module doc): bindings with a proven view
/// the loop never writes, with the ends of their proven-index accesses. An
/// access whose index has no range, or whose symbolic end names a second
/// bound, is no bare access (the planner stales the facts there).
fn view_candidates(
    ctx: &FnCtx<'_>,
    body: &[Stmt],
    env: &Env,
    written: &HashSet<u32>,
) -> HashMap<Recv, ArrayUse> {
    let mut out: HashMap<Recv, ArrayUse> = HashMap::new();
    if !view_regions_enabled() {
        return out;
    }
    let mut accesses: Vec<(u32, ViewEnd)> = Vec::new();
    let mut walk = |e: &Expr| -> bool {
        fn e_walk(e: &Expr, env: &Env, out: &mut Vec<(u32, ViewEnd)>) {
            let access = match e {
                Expr::IndexGet { object, index } => Some((object.as_ref(), index.as_ref())),
                _ => element_store(e).map(|(o, i, _)| (o, i)),
            };
            if let Some((Expr::LocalGet(id), index)) = access {
                if let Some(end) = env.view_end(index) {
                    out.push((*id, end));
                }
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, env, out));
        }
        e_walk(e, env, &mut accesses);
        false
    };
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut walk);
    }
    for (id, end) in accesses {
        if written.contains(&id) || view_of(ctx, id).is_none() {
            continue;
        }
        let u = out.entry(Recv::Local(id)).or_insert(ArrayUse {
            view: true,
            ..ArrayUse::default()
        });
        match end.end {
            End::Const(e) => u.view_end = u.view_end.max(e as u32),
            End::Sym(b, c) => match u.view_sym {
                None => u.view_sym = Some((b, c)),
                Some((sb, sc)) if sb == b => u.view_sym = Some((b, sc.max(c))),
                Some(_) => continue,
            },
        }
        u.view_counter |= end.counter;
    }
    // A view whose length is a compile-time constant covering every
    // constant end is already proven by the static range proof
    // (`bounds_for_buffer_access_width`); a guard would add nothing.
    out.retain(|r, u| {
        let Recv::Local(id) = r else { return true };
        let constant = view_of(ctx, *id).and_then(|v| match v.length_source {
            Some(crate::native_value::LengthSource::Constant(n)) => Some(n),
            _ => None,
        });
        !(u.view_sym.is_none() && constant.is_some_and(|n| i64::from(u.view_end) <= n))
    });
    out
}

/// Does `s` contain an element access of a binding with a view?
fn has_view_access(ctx: &FnCtx<'_>, s: &Stmt) -> bool {
    perry_hir::walker::stmt_any_expr(s, &mut |e| {
        fn walk(ctx: &FnCtx<'_>, e: &Expr) -> bool {
            let hit = match e {
                Expr::IndexGet { object, .. } => {
                    matches!(object.as_ref(), Expr::LocalGet(id) if view_of(ctx, *id).is_some())
                }
                _ => element_store(e).is_some_and(
                    |(o, _, _)| matches!(o, Expr::LocalGet(id) if view_of(ctx, *id).is_some()),
                ),
            };
            let mut found = hit;
            if !found {
                perry_hir::walker::walk_expr_children(e, &mut |c| found |= walk(ctx, c));
            }
            found
        }
        walk(ctx, e)
    })
}

/// A view run: the straight-line statements (`let` / expression statements)
/// at the head of `stmts`, starting with a view access, through the last
/// one with a bare view access. It is the loop region's view proof without
/// the loop (an unrolled loop, `perry_transform::unroll_static_loops`, or a
/// run of accesses of its own): one guard, then F (bare) and G (today's).
/// Returns the run's plan when it pays.
pub(super) fn view_run_plan(ctx: &FnCtx<'_>, stmts: &[Stmt]) -> Option<(usize, Plan, Env)> {
    if !view_regions_enabled() || stmts.is_empty() || !has_view_access(ctx, &stmts[0]) {
        return None;
    }
    let len = stmts
        .iter()
        .take_while(|s| matches!(s, Stmt::Let { .. } | Stmt::Expr(_)))
        .count();
    if len < 2 {
        return None;
    }
    let mut run = &stmts[..len];
    let written = assigned(run, &[]);
    let env = Env {
        counter: None,
        aliases: HashMap::new(),
        ranges: view_ranges(ctx, None, run, &written),
    };
    let arrs = view_candidates(ctx, run, &env, &written);
    if arrs.is_empty() {
        return None;
    }
    let p = plan(
        ctx,
        run,
        HashSet::new(),
        &HashMap::new(),
        arrs.clone(),
        &env,
        None,
        None,
    )?;
    // Through the last statement with a bare access.
    let last = run.iter().rposition(|s| {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            fn any(e: &Expr, bare: &HashSet<usize>) -> bool {
                let mut f = bare.contains(&(e as *const Expr as usize));
                if !f {
                    perry_hir::walker::walk_expr_children(e, &mut |c| f |= any(c, bare));
                }
                f
            }
            any(e, &p.bare)
        })
    })?;
    if last + 1 < run.len() {
        run = &run[..last + 1];
    }
    let p = if run.len() == len {
        p
    } else {
        plan(
            ctx,
            run,
            HashSet::new(),
            &HashMap::new(),
            arrs,
            &env,
            None,
            None,
        )?
    };
    // One guard (a load and a compare per view) pays for itself from a few
    // checked accesses on.
    if p.view_index.len() < 4 || !pays(ctx, "view-run", body_nodes(run), p.view_index.len()) {
        return None;
    }
    Some((run.len(), p, env))
}

/// Is the element access at `index` into `buffer_local_id` a planned-bare
/// VIEW access of the active region? Its bounds are then proven by the
/// region's guard (`BoundsProof::RegionGuard`).
pub(crate) fn view_bounds_proven(ctx: &FnCtx<'_>, buffer_local_id: u32, index: &Expr) -> bool {
    ctx.region_loop_facts.last().is_some_and(|a| {
        a.view_index.contains(&(index as *const Expr as usize))
            && a.arrays
                .iter()
                .any(|x| x.view.is_some() && x.recv == Recv::Local(buffer_local_id))
    })
}

/// Record a bare VIEW access about to be emitted at the current position,
/// for [`verify`](super::verify): no JS-capable call may reach it.
pub(crate) fn note_view_access(ctx: &mut FnCtx<'_>) {
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted.push((b, i));
    }
    stat(2, 1);
}

/// Load a symbolic bound at every guard/re-check. Even a sealed view's
/// guard uses a plain load; only its ordinary length reads may be invariant.
fn emit_symbol(ctx: &mut FnCtx<'_>, s: Symbol) -> Result<String> {
    match s {
        Symbol::Local(id) => lower_expr(ctx, &Expr::LocalGet(id)),
        Symbol::ViewLength(id, sub) => {
            let v = view_of(ctx, id).expect("a length bound has a proven view");
            let subtract = sub
                .map(|k| lower_expr(ctx, &Expr::LocalGet(k)))
                .transpose()?;
            let blk = ctx.block();
            let data = blk.load(crate::types::PTR, &v.data_slot);
            let ptr = blk.gep(I8, &data, &[(I32, &v.length_offset_from_data.to_string())]);
            let len = blk.load(I32, &ptr);
            // Match .length's unsigned u32 interpretation. A signed source
            // could compare equal to a negative target length and admit a
            // view larger than the signed index domain.
            let len = blk.uitofp(I32, &len, DOUBLE);
            Ok(match subtract {
                Some(k) => blk.fsub(&len, &k),
                None => len,
            })
        }
    }
}

/// The guard of a view receiver (see [`ViewGuard`]); `counter_ok` is the
/// counter's entry check.
fn emit_view_guard(ctx: &mut FnCtx<'_>, v: &ViewGuard, counter_ok: &str) -> Result<String> {
    let sym = match v.sym {
        Some((b, c)) => Some((emit_symbol(ctx, b)?, c)),
        None => None,
    };
    let blk = ctx.block();
    let data = blk.load(crate::types::PTR, &v.data_slot);
    let len_ptr = blk.gep(I8, &data, &[(I32, &v.length_offset.to_string())]);
    // A plain load: the length changes when the buffer is detached.
    let len = blk.load(I32, &len_ptr);
    let mut ok = counter_ok.to_string();
    if v.end > 0 {
        let c = blk.icmp_ule(I32, &v.end.to_string(), &len);
        ok = blk.and(I1, &ok, &c);
    }
    if let Some((bv, c)) = sym {
        // `B + c <= length` as doubles: a non-Number `B` is a NaN and fails.
        let lim = blk.fadd(&bv, &format!("{:?}", c as f64));
        let lenf = blk.sitofp(I32, &len, DOUBLE);
        let c = blk.fcmp("ole", &lim, &lenf);
        ok = blk.and(I1, &ok, &c);
    }
    Ok(ok)
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
            Bound::Lit(k) => Some(format!("{:?}", k as f64)),
            Bound::Local(id) => Some(lower_expr(ctx, &Expr::LocalGet(id))?),
            Bound::ViewLength(..) => {
                assert!(a.view.is_some(), "length counters require a view guard");
                None
            }
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
        if let Bound::ViewLength(id, sub, c) = c.bound {
            // The iteration's condition preceded a possible JS call. A
            // shrinking length can invalidate that condition mid-iteration:
            // end <= target length alone then admits an already-outside i.
            // Re-establish i < the current exclusive loop end as well.
            let end = emit_symbol(ctx, Symbol::ViewLength(id, sub))?;
            let blk = ctx.block();
            let end = if c == 0 {
                end
            } else {
                blk.fadd(&end, &format!("{:?}", c as f64))
            };
            let below = blk.fcmp("olt", &iv, &end);
            counter_ok = blk.and(I1, &counter_ok, &below);
        }
        bound = b;
    }
    if let Some(v) = &a.view {
        return emit_view_guard(ctx, v, &counter_ok);
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
    let Some(ar) = a
        .arrays
        .iter()
        .find(|x| x.view.is_none() && x.is(object))
        .cloned()
    else {
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
    let Some(ar) = a
        .arrays
        .iter()
        .find(|x| x.view.is_none() && x.is(object))
        .cloned()
    else {
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
    let Expr::IndexGet { object, .. } = e else {
        return false;
    };
    // A VIEW access lowers through the typed-array tiers themselves.
    ctx.region_loop_facts.last().is_some_and(|a| {
        a.bare.contains(&(e as *const Expr as usize))
            && a.arrays.iter().any(|x| x.view.is_none() && x.is(object))
    })
}

/// The loop poll's arm: a collection may have moved every region array, so
/// each base whose region is valid is re-derived from the binding's root.
pub(crate) fn emit_poll_refresh(ctx: &mut FnCtx<'_>) -> Result<()> {
    let recipes: Vec<(String, ArrayRecv)> = ctx
        .region_loops
        .iter()
        .filter_map(|p| p.valid_slot.clone().map(|v| (v, p.arrays.clone())))
        .flat_map(|(v, arrs)| arrs.into_iter().map(move |a| (v.clone(), a)))
        // A view keeps no base: its storage never moves.
        .filter(|(_, a)| a.view.is_none())
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
        if a.view_index.contains(&o) && a.view_index.insert(c) {
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
            a.view_index.remove(&p);
        }
    }
}

#[cfg(test)]
#[path = "arrays_tests.rs"]
mod tests;
