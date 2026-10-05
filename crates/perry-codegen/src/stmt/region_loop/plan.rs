//! Step 4b regions: the planner (which accesses are bare, the re-check a loop needs).

use super::*;

// ---------------------------------------------------------------- planning

/// Facts hold on every path.
pub(super) const FRESH: u8 = 2;
/// Facts hold on every path except the generic arm of a fact tree, which
/// sets the region's dirty flag: the next iteration re-checks only if it was
/// taken. Not enough for a bare access.
pub(super) const DIRTY: u8 = 1;

/// `None` = unreachable. Absent receiver = stale.
pub(super) type St = Option<BTreeMap<Recv, u8>>;

pub(super) fn meet(a: St, b: St) -> St {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(
            a.into_iter()
                .filter_map(|(k, v)| b.get(&k).map(|w| (k, v.min(*w))))
                .collect(),
        ),
    }
}

pub(super) fn dirty(st: &mut St) {
    if let Some(m) = st {
        for v in m.values_mut() {
            *v = (*v).min(DIRTY);
        }
    }
}

/// A `+` tree over reads of ONE receiver's region keys, locals and numeric
/// literals (at least one read): lowered as a FACT TREE — the reads are bare
/// loads, every leaf is verified a Number, the tree folds to `fadd`s, and a
/// failed check runs the tree through today's lowering in source order (with
/// the facts masked) and sets the dirty flag. Slice 1's rule (§L7.3) with the
/// region's facts in place of its own guard.
pub(super) fn fact_tree_leaves<'e>(
    e: &'e Expr,
    covered: &dyn Fn(Recv, &str) -> bool,
) -> Option<(Recv, Vec<&'e Expr>)> {
    fn leaves<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            leaves(left, out);
            leaves(right, out);
        } else {
            out.push(e);
        }
    }
    if !matches!(
        e,
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ) {
        return None;
    }
    let mut all = Vec::new();
    leaves(e, &mut all);
    let mut recv: Option<Recv> = None;
    let mut reads = Vec::new();
    for l in all {
        match l {
            Expr::PropertyGet {
                object, property, ..
            } => {
                let r = Recv::of(object)?;
                if recv.is_some_and(|x| x != r) || !covered(r, property) {
                    return None;
                }
                recv = Some(r);
                reads.push(l);
            }
            Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) => {}
            _ => return None,
        }
    }
    Some((recv?, reads))
}

/// Candidate reads inside a Number-consuming expression. The planner's
/// exact bare-access set filters these by freshness after the walk.
///
/// Only a value the operator itself consumes is a candidate. A read beneath
/// another property access is that access's RECEIVER (`o.a.length` reads
/// `o.a` as an object), and a read beneath an element access or a call is
/// that operation's key, receiver or argument: none of them is a Number
/// operand, so none may ask the region for a Number lane (R). Nor is a value
/// only tested for truthiness: a conditional's test (`o.a ? 1 : 0`; its arms
/// are the values) and a `!` operand.
fn number_operand_reads(e: &Expr, out: &mut Vec<(usize, Recv, String)>) {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } => {
            if let Some(r) = Recv::of(object) {
                out.push((e as *const Expr as usize, r, property.clone()));
            }
            return;
        }
        Expr::IndexGet { .. } | Expr::Call { .. } | Expr::CallSpread { .. } | Expr::New { .. } => {
            return
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            number_operand_reads(then_expr, out);
            number_operand_reads(else_expr, out);
            return;
        }
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => return,
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| number_operand_reads(child, out));
}

/// Local value operands whose incoming Number-ness can be discharged by
/// the region's strict entry tests. A receiver beneath PropertyGet is an
/// object, not a candidate Number value; so is the receiver of an element
/// read, and its key is a property key (the read's VALUE is the operand).
fn number_operand_locals(e: &Expr, out: &mut HashSet<u32>) {
    match e {
        Expr::LocalGet(id) => {
            out.insert(*id);
            return;
        }
        Expr::PropertyGet { .. } | Expr::IndexGet { .. } => return,
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| number_operand_locals(child, out));
}

pub(super) fn kill(st: &mut St) {
    if let Some(m) = st {
        m.clear();
    }
}

/// A leaf that is a primitive by construction: an operator over it cannot
/// reach ToPrimitive.
pub(super) fn prim(e: &Expr) -> bool {
    match e {
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::TypeOf(_) => true,
        Expr::Binary { op, left, right } => {
            !matches!(op, BinaryOp::Add) || (prim(left) && prim(right))
        }
        Expr::Compare { .. } => true,
        Expr::Unary { op, operand } => matches!(op, UnaryOp::Not) || prim(operand),
        _ => false,
    }
}

pub(super) struct Planner<'p, 'a> {
    ctx: &'p FnCtx<'a>,
    cands: &'p HashSet<Recv>,
    keys: &'p HashMap<Recv, Vec<String>>,
    /// Array receivers (S3) and what their accesses need.
    arrays: &'p HashMap<Recv, ArrayUse>,
    /// The loop counter and body-local copies an index proof may use.
    env: &'p Env,
    bare: HashSet<usize>,
    /// Potential R keys, filtered against exact fresh bare reads at finish.
    number_reads: Vec<(usize, Recv, String)>,
    number_local_uses: HashSet<u32>,
    bare_reads: Vec<(usize, Recv, String)>,
    bare_arrays: HashSet<Recv>,
    /// The index expressions of bare VIEW accesses (their bounds are the
    /// region guard's, `arrays::view_bounds_proven`).
    view_index: HashSet<usize>,
    /// Bare VIEW element reads (Numbers by construction: the 5L fixed
    /// point's leaves), and every VIEW read a guard covers whether or not
    /// it is reached fresh (the fixed point's optimistic start).
    view_reads: Vec<usize>,
    view_cands: Vec<usize>,
    /// A body region nested in this loop region (array regions): while its
    /// tail is walked, its bare accesses run no JS and its fact trees only
    /// set the loop's dirty flag.
    inner: Option<(&'p HashSet<usize>, &'p HashSet<usize>, &'p HashSet<usize>)>,
    in_inner: bool,
    /// While the nested body region's tail is walked outside a nested
    /// loop's first pass: a statement there after which the facts may be
    /// stale is recorded in `dirty_after` (the tail's own accesses are not
    /// recorded; they lower under the body region's facts).
    record_dirty: bool,
    /// A store RHS can use R to prove its own numeric reads. Its add must
    /// stay an ordinary bare-read add, not the generic fact tree (which
    /// dirties freshness before the following store).
    in_store_rhs: bool,
    trees: HashSet<usize>,
    bare_stores: HashSet<Recv>,
    /// Per receiver, the keys a planned-bare store may write a value the
    /// compiler does not prove a canonical double into (charter step 5): the
    /// runtime then refuses a word whose shape has a non-`Any` lane there.
    boxed_stores: HashMap<Recv, HashSet<String>>,
    continues: Vec<St>,
    record: bool,
    /// Exact R reads and scoped 5L locals proved by an earlier planning pass.
    /// A read is usable only after this pass has itself made it bare.
    proof_reads: &'p HashSet<usize>,
    proof_locals: &'p HashSet<u32>,
    /// #10741: a statement after which the facts may be stale sets the
    /// region's dirty flag (lowering stores it right after the statement),
    /// so the facts are DIRTY, not lost, from there to the back edge: the
    /// next iteration re-checks only when such a statement ran, instead of
    /// the loop being refused or re-checked every iteration.
    mark_dirty: bool,
    dirty_after: HashSet<usize>,
    /// The expressions this walk judged able to run JS (recorded walks only).
    stale_at: std::cell::RefCell<HashSet<usize>>,
}

impl Planner<'_, '_> {
    /// `e` may run JS: every fact is stale. `PERRY_REGION_DIAG=5` names
    /// the expression that staled them (the planner's refusal trace).
    fn stale(&self, e: &Expr, st: &mut St) {
        // In a nested body region's tail, that region's own walk is the judge
        // of what can run JS: it proves operands this walk cannot (its R
        // reads and the locals they feed), so an expression it walked without
        // staling its facts runs no JS. Only its stale points touch the
        // loop's facts.
        if self.in_inner {
            if let Some((_, _, istale)) = self.inner {
                if !istale.contains(&(e as *const Expr as usize)) {
                    return;
                }
            }
        }
        if self.record {
            self.stale_at.borrow_mut().insert(e as *const Expr as usize);
        }
        if st.as_ref().is_some_and(|m| !m.is_empty())
            && std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("5")
        {
            let d = format!("{e:?}");
            eprintln!(
                "[perry region] stale at {} in {}",
                &d[..d.len().min(160)],
                self.ctx.func.name
            );
        }
        kill(st);
    }
    /// A primitive by construction, or a value the compiler proves a raw
    /// double (`expr_produces_canonical_raw_f64` is the predicate that already
    /// licenses an unguarded `fadd`).
    fn prim(&self, e: &Expr) -> bool {
        match e {
            Expr::PropertyGet { .. } => {
                if arrays::native_view_length(self.ctx, e) {
                    return true;
                }
                let ptr = e as *const Expr as usize;
                self.bare.contains(&ptr) && self.proof_reads.contains(&ptr)
            }
            Expr::LocalGet(id) if self.proof_locals.contains(id) => true,
            // A bare element read of a dense raw-f64 array: a double.
            Expr::IndexGet { .. } => self.f64_element(e) || self.view_element(e),
            Expr::Binary { left, right, .. } => self.prim(left) && self.prim(right),
            Expr::Unary { op, operand } if !matches!(op, UnaryOp::Not) => self.prim(operand),
            _ => prim(e) || crate::type_analysis::is_numeric_expr(self.ctx, e),
        }
    }

    /// A planned-bare element read of a dense region array.
    fn f64_element(&self, e: &Expr) -> bool {
        let Expr::IndexGet { object, .. } = e else {
            return false;
        };
        self.bare.contains(&(e as *const Expr as usize))
            && self
                .env
                .array(object)
                .and_then(|r| self.arrays.get(&r))
                .is_some_and(|u| u.dense())
    }

    /// A planned-bare element read of a VIEW receiver: a Number (every
    /// proven view kind holds Numbers; a float lane is canonicalised).
    fn view_element(&self, e: &Expr) -> bool {
        let Expr::IndexGet { object, .. } = e else {
            return false;
        };
        self.bare.contains(&(e as *const Expr as usize))
            && self
                .env
                .array(object)
                .and_then(|r| self.arrays.get(&r))
                .is_some_and(|u| u.view)
    }

    /// A bare VIEW access at `index` of `r`, with the facts fresh?
    fn view_access(&self, r: Recv, index: &Expr, st: &St) -> bool {
        self.arrays.get(&r).is_some_and(|u| u.view)
            && self
                .env
                .view_end(index)
                .is_some_and(|end| self.arrays[&r].view_covers(end))
            && st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH))
    }

    /// A canonical double by construction, for a bare element STORE: the
    /// mirror of `expr_produces_canonical_raw_f64` over this plan's bare
    /// element reads (lowering asks that predicate again with the facts
    /// active). Locals count only through the static predicate — never
    /// through a region entry assumption.
    fn num(&self, e: &Expr) -> bool {
        match e {
            Expr::Number(_) | Expr::Integer(_) => true,
            Expr::IndexGet { .. } => self.f64_element(e) || self.view_element(e),
            // Every binary operator over two Numbers yields a Number.
            Expr::Binary { left, right, .. } => self.num(left) && self.num(right),
            Expr::Unary { op, operand } => {
                matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot) && self.num(operand)
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => self.num(then_expr) && self.num(else_expr),
            _ if pure_math_args(e).is_some() => {
                pure_math_args(e).is_some_and(|args| args.iter().all(|a| self.num(a)))
            }
            _ => crate::type_analysis::expr_produces_canonical_raw_f64(self.ctx, e),
        }
    }

    /// After its operands: is the element store `e` bare? Else the facts
    /// are stale.
    fn element_store(&mut self, e: &Expr, st: &mut St) {
        let Some((object, index, value)) = arrays::element_store(e) else {
            return self.stale(e, st);
        };
        // A VIEW store of a primitive: ToNumber of a primitive runs no JS,
        // and an element store changes no length.
        if let Some(r) = self.env.array(object) {
            if self.arrays.get(&r).is_some_and(|u| u.view) {
                // ...and only a value the native element store takes (else
                // the store is a runtime call).
                let native = match r {
                    Recv::Local(id) => arrays::view_of(self.ctx, id).is_some_and(|v| {
                        crate::expr::typed_array_store_value_is_native(self.ctx, &v, value)
                    }),
                    Recv::This => false,
                };
                if self.view_access(r, index, st) && self.prim(value) && native {
                    if self.record {
                        self.bare.insert(e as *const Expr as usize);
                        self.view_index.insert(index as *const Expr as usize);
                        self.bare_arrays.insert(r);
                    }
                } else {
                    self.stale(e, st);
                }
                return;
            }
        }
        let r = self
            .env
            .array(object)
            .filter(|r| self.arrays.get(r).is_some_and(|u| u.dense() && u.store));
        match r {
            Some(r)
                if self.env.index(index).is_some()
                    && self.num(value)
                    && st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH)) =>
            {
                if self.record {
                    self.bare.insert(e as *const Expr as usize);
                    self.bare_arrays.insert(r);
                }
            }
            _ => self.stale(e, st),
        }
    }

    fn covered(&self, r: Recv, key: &str) -> bool {
        self.keys
            .get(&r)
            .is_some_and(|k| k.iter().any(|x| x == key))
    }

    /// `boxed`: a store whose value is not proven a canonical double.
    fn access(&mut self, e: &Expr, r: Recv, key: &str, store: bool, boxed: bool, st: &mut St) {
        let fresh = st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH));
        if fresh && self.cands.contains(&r) && self.covered(r, key) {
            if self.record {
                let ptr = e as *const Expr as usize;
                self.bare.insert(ptr);
                if !store {
                    self.bare_reads.push((ptr, r, key.to_string()));
                }
                if store {
                    self.bare_stores.insert(r);
                    if boxed {
                        self.boxed_stores
                            .entry(r)
                            .or_default()
                            .insert(key.to_string());
                    }
                }
            }
            return;
        }
        // Today's tower: its miss path can reach a getter/setter or reshape
        // the receiver.
        self.stale(e, st);
    }

    fn exprs(&mut self, es: &[Expr], mut st: St) -> St {
        for e in es {
            st = self.expr(e, st);
        }
        st
    }

    fn expr(&mut self, e: &Expr, mut st: St) -> St {
        st.as_ref()?;
        if self.in_inner {
            if let Some((ibare, itrees, _)) = self.inner {
                let key = e as *const Expr as usize;
                if itrees.contains(&key) {
                    dirty(&mut st);
                    return st;
                }
                if ibare.contains(&key) {
                    if let Expr::PutValueSet { value, .. } | Expr::PropertySet { value, .. } = e {
                        return self.expr(value, st);
                    }
                    return st;
                }
            }
        }
        match e {
            Expr::PropertyGet {
                object, property, ..
            } => {
                st = self.expr(object, st);
                if arrays::native_view_length(self.ctx, e) {
                    return st;
                }
                match Recv::of(object) {
                    Some(r) => self.access(e, r, property, false, false, &mut st),
                    None => self.stale(e, &mut st),
                }
                st
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                st = self.expr(target, st);
                st = self.expr(key, st);
                let outer_rhs = self.in_store_rhs;
                self.in_store_rhs = true;
                st = self.expr(value, st);
                self.in_store_rhs = outer_rhs;
                match (Recv::of(target), key.as_ref()) {
                    (Some(r), Expr::String(k)) if Recv::of(receiver) == Some(r) => {
                        // The same predicate the bare store lowers with
                        // (`bare::try_lower_bare_put`): a proven canonical
                        // double is a valid value of every lane.
                        let boxed =
                            !crate::type_analysis::expr_produces_canonical_raw_f64(self.ctx, value);
                        self.access(e, r, k, true, boxed, &mut st)
                    }
                    _ if arrays::element_store(e).is_some() => self.element_store(e, &mut st),
                    _ => self.stale(e, &mut st),
                }
                st
            }
            // A static-key store through a binding: `o.p = v` in the forms
            // that lower to `PropertySet`, and `o.p op= v` on a `const`
            // binding (no base temp). The same bare store as `PutValueSet`'s.
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                st = self.expr(object, st);
                let outer_rhs = self.in_store_rhs;
                self.in_store_rhs = true;
                st = self.expr(value, st);
                self.in_store_rhs = outer_rhs;
                match Recv::of(object) {
                    Some(r) => {
                        let boxed =
                            !crate::type_analysis::expr_produces_canonical_raw_f64(self.ctx, value);
                        self.access(e, r, property, true, boxed, &mut st)
                    }
                    None => self.stale(e, &mut st),
                }
                st
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                    st = self.expr(object, st);
                } else {
                    st = self.expr(callee, st);
                }
                st = self.exprs(args, st);
                self.stale(e, &mut st);
                st
            }
            Expr::LocalSet(_, v) => self.expr(v, st),
            // An element read of a region array with a static index range is
            // bare while the facts are fresh: it runs no JS, so it stales
            // nothing. Any other element read is today's tower.
            Expr::IndexGet { object, index } => {
                st = self.expr(object, st);
                st = self.expr(index, st);
                let r = self
                    .env
                    .array(object)
                    .filter(|r| self.arrays.contains_key(r));
                if let Some(r) = r.filter(|r| self.arrays[r].view) {
                    let covered = self
                        .env
                        .view_end(index)
                        .is_some_and(|end| self.arrays[&r].view_covers(end));
                    if covered && self.record {
                        self.view_cands.push(e as *const Expr as usize);
                    }
                    if self.view_access(r, index, &st) {
                        if self.record {
                            self.view_reads.push(e as *const Expr as usize);
                            self.bare.insert(e as *const Expr as usize);
                            self.view_index
                                .insert(index.as_ref() as *const Expr as usize);
                            self.bare_arrays.insert(r);
                        }
                    } else {
                        self.stale(e, &mut st);
                    }
                    return st;
                }
                match r {
                    Some(r)
                        if self.env.index(index).is_some()
                            && st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH)) =>
                    {
                        if self.record {
                            self.bare.insert(e as *const Expr as usize);
                            self.bare_arrays.insert(r);
                        }
                    }
                    _ => self.stale(e, &mut st),
                }
                st
            }
            // #10741: an element store into a dense region array at a proven
            // index, of a value proven a canonical double, runs no JS and
            // changes no fact (not the length, not the layout). Anything
            // else is today's store, which can.
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                st = self.expr(object, st);
                st = self.expr(index, st);
                st = self.expr(value, st);
                self.element_store(e, &mut st);
                st
            }
            // Pure `Math.*` over primitives runs no JS (a spread argument
            // iterates, so it is not one of these).
            _ if pure_math_args(e).is_some() => {
                let args = pure_math_args(e).unwrap_or_default();
                for a in &args {
                    st = self.expr(a, st);
                }
                if !args.iter().all(|a| self.prim(a)) {
                    self.stale(e, &mut st);
                }
                st
            }
            Expr::Binary { left, right, .. } => {
                if self.record {
                    number_operand_reads(left, &mut self.number_reads);
                    number_operand_reads(right, &mut self.number_reads);
                    number_operand_locals(left, &mut self.number_local_uses);
                    number_operand_locals(right, &mut self.number_local_uses);
                }
                let keys = self.keys;
                let cands = self.cands;
                let covered = |r: Recv, k: &str| {
                    cands.contains(&r) && keys.get(&r).is_some_and(|l| l.iter().any(|x| x == k))
                };
                if !self.in_store_rhs {
                    if let Some((r, reads)) = fact_tree_leaves(e, &covered) {
                        if st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH)) {
                            if self.record {
                                self.trees.insert(e as *const Expr as usize);
                                for l in reads {
                                    let ptr = l as *const Expr as usize;
                                    self.bare.insert(ptr);
                                    if let Expr::PropertyGet {
                                        object, property, ..
                                    } = l
                                    {
                                        if let Some(r) = Recv::of(object) {
                                            self.bare_reads.push((ptr, r, property.clone()));
                                        }
                                    }
                                }
                            }
                            dirty(&mut st);
                            return st;
                        }
                    }
                }
                st = self.expr(left, st);
                st = self.expr(right, st);
                if !(self.prim(left) && self.prim(right)) {
                    self.stale(e, &mut st);
                }
                st
            }
            Expr::Unary { op, operand } => {
                if self.record && !matches!(op, UnaryOp::Not) {
                    number_operand_reads(operand, &mut self.number_reads);
                    number_operand_locals(operand, &mut self.number_local_uses);
                }
                st = self.expr(operand, st);
                if !matches!(op, UnaryOp::Not) && !self.prim(operand) {
                    self.stale(e, &mut st);
                }
                st
            }
            Expr::Compare { op, left, right } => {
                if self.record
                    && matches!(
                        op,
                        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                    )
                {
                    number_operand_reads(left, &mut self.number_reads);
                    number_operand_reads(right, &mut self.number_reads);
                    number_operand_locals(left, &mut self.number_local_uses);
                    number_operand_locals(right, &mut self.number_local_uses);
                }
                st = self.expr(left, st);
                st = self.expr(right, st);
                if !matches!(op, CompareOp::Eq | CompareOp::Ne)
                    && !(self.prim(left) && self.prim(right))
                {
                    self.stale(e, &mut st);
                }
                st
            }
            Expr::Logical { left, right, .. } => {
                let a = self.expr(left, st);
                let b = self.expr(right, a.clone());
                meet(a, b)
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let c = self.expr(condition, st);
                let t = self.expr(then_expr, c.clone());
                let f = self.expr(else_expr, c);
                meet(t, f)
            }
            Expr::Sequence(v) => self.exprs(v, st),
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
            | Expr::FuncRef(_)
            | Expr::This => st,
            // A local ++/-- ToNumerics its operand (valueOf on an object).
            // A ranged local (a view proof's counter) is an integer Number.
            Expr::Update { id, .. } => {
                if !self.ctx.integer_locals.contains(id) && !self.env.ranges.contains_key(id) {
                    self.stale(e, &mut st);
                }
                st
            }
            Expr::TypeOf(x) | Expr::Void(x) => self.expr(x, st),
            _ => {
                // Everything else is treated as able to run JS: the planner is
                // conservative, and `verify` is the check.
                let mut kids: Vec<&Expr> = Vec::new();
                perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
                for k in kids {
                    st = self.expr(k, st);
                }
                self.stale(e, &mut st);
                st
            }
        }
    }

    fn stmts(&mut self, ss: &[Stmt], mut st: St) -> St {
        for s in ss {
            let before = if self.mark_dirty { st.clone() } else { None };
            st = self.stmt(s, st);
            if let (Some(b), Some(a)) = (&before, &st) {
                if b.keys().any(|r| !a.contains_key(r)) {
                    if self.record || self.record_dirty {
                        self.dirty_after.insert(s as *const Stmt as usize);
                    }
                    st = Some(b.keys().map(|r| (*r, DIRTY)).collect());
                }
            }
        }
        st
    }

    fn stmt(&mut self, s: &Stmt, mut st: St) -> St {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    st = self.expr(e, st);
                }
                st
            }
            Stmt::Expr(e) => self.expr(e, st),
            Stmt::Throw(e) => {
                let _ = self.expr(e, st);
                None
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let _ = self.expr(e, st);
                }
                None
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let c = self.expr(condition, st);
                let t = self.stmts(then_branch, c.clone());
                let f = match else_branch {
                    Some(b) => self.stmts(b, c),
                    None => c,
                };
                meet(t, f)
            }
            Stmt::Break => None,
            Stmt::Continue => {
                self.continues.push(st);
                None
            }
            // Nested loops: two passes (the second records) from the meet of
            // the entry and the back edge; `break`/`continue` inside them
            // belong to them.
            Stmt::While { condition, body } => self.inner_loop(Some(condition), body, None, st),
            Stmt::DoWhile { body, condition } => self.inner_loop(Some(condition), body, None, st),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    st = self.stmt(i, st);
                }
                self.inner_loop(condition.as_ref(), body, update.as_ref(), st)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                // `break` inside a switch leaves the switch; conservatively
                // everything after a switch is stale.
                let d = self.expr(discriminant, st);
                let saved = std::mem::take(&mut self.continues);
                let mut prev = d.clone();
                for c in cases {
                    if let Some(t) = &c.test {
                        prev = self.expr(t, prev);
                    }
                    prev = self.stmts(&c.body, prev);
                }
                let inner = std::mem::replace(&mut self.continues, saved);
                self.continues.extend(inner);
                d.map(|_| BTreeMap::new())
            }
            _ => {
                kill(&mut st);
                st
            }
        }
    }

    fn inner_loop(
        &mut self,
        cond: Option<&Expr>,
        body: &[Stmt],
        update: Option<&Expr>,
        st: St,
    ) -> St {
        let rec = self.record;
        let rec_dirty = self.record_dirty;
        let saved = std::mem::take(&mut self.continues);
        self.record = false;
        self.record_dirty = false;
        let mut s = st.clone();
        if let Some(c) = cond {
            s = self.expr(c, s);
        }
        let end = self.stmts(body, s);
        let conts = std::mem::take(&mut self.continues);
        let mut back = end;
        for c in conts {
            back = meet(back, c);
        }
        if let Some(u) = update {
            back = self.expr(u, back);
        }
        self.record = rec;
        self.record_dirty = rec_dirty;
        let head = meet(st.clone(), back);
        let mut s = head.clone();
        if let Some(c) = cond {
            s = self.expr(c, s);
        }
        let _ = self.stmts(body, s.clone());
        self.continues = saved;
        // `break` can leave from anywhere: the exit is stale.
        s.map(|_| BTreeMap::new())
    }
}

/// The arguments of a `Math.*` call that runs no JavaScript once its
/// arguments are primitives (`ToNumber` of a primitive cannot call out);
/// `None` for anything else, including the spread forms (they iterate).
pub(super) fn pure_math_args(e: &Expr) -> Option<Vec<&Expr>> {
    match e {
        Expr::MathFloor(..)
        | Expr::MathCeil(..)
        | Expr::MathRound(..)
        | Expr::MathTrunc(..)
        | Expr::MathSign(..)
        | Expr::MathAbs(..)
        | Expr::MathSqrt(..)
        | Expr::MathLog(..)
        | Expr::MathLog2(..)
        | Expr::MathLog10(..)
        | Expr::MathPow(..)
        | Expr::MathMin(..)
        | Expr::MathMax(..)
        | Expr::MathImul(..)
        | Expr::MathRandom
        | Expr::MathSin(..)
        | Expr::MathCos(..)
        | Expr::MathTan(..)
        | Expr::MathAsin(..)
        | Expr::MathAcos(..)
        | Expr::MathAtan(..)
        | Expr::MathAtan2(..)
        | Expr::MathCbrt(..)
        | Expr::MathHypot(..)
        | Expr::MathFround(..)
        | Expr::MathF16round(..)
        | Expr::MathClz32(..)
        | Expr::MathExpm1(..)
        | Expr::MathLog1p(..)
        | Expr::MathSinh(..)
        | Expr::MathCosh(..)
        | Expr::MathTanh(..)
        | Expr::MathAsinh(..)
        | Expr::MathAcosh(..)
        | Expr::MathAtanh(..)
        | Expr::MathExp(..) => {
            let mut args = Vec::new();
            perry_hir::walker::walk_expr_children(e, &mut |c| args.push(c));
            Some(args)
        }
        _ => None,
    }
}

/// Anything that makes a body unsuitable for a split: a closure (a second
/// lowering would emit it twice), `try` (handlers), labels, generators.
pub(super) fn body_refused(ss: &[Stmt]) -> bool {
    fn e_bad(e: &Expr) -> bool {
        if matches!(
            e,
            Expr::Closure { .. } | Expr::Yield { .. } | Expr::Await(_)
        ) {
            return true;
        }
        let mut bad = false;
        perry_hir::walker::walk_expr_children(e, &mut |c| bad |= e_bad(c));
        bad
    }
    fn s_bad(s: &Stmt) -> bool {
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_bad(e),
            Stmt::Return(Some(e)) => e_bad(e),
            Stmt::Let { .. } | Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_bad(condition)
                    || then_branch.iter().any(s_bad)
                    || else_branch.as_ref().is_some_and(|b| b.iter().any(s_bad))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_bad(condition) || body.iter().any(s_bad)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| s_bad(i))
                    || condition.as_ref().is_some_and(e_bad)
                    || update.as_ref().is_some_and(e_bad)
                    || body.iter().any(s_bad)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_bad(discriminant)
                    || cases
                        .iter()
                        .any(|c| c.test.as_ref().is_some_and(e_bad) || c.body.iter().any(s_bad))
            }
            _ => true,
        }
    }
    ss.iter().any(s_bad)
}

/// The HIR size of `ss` (statements plus expressions): what a region copies
/// when it versions a loop or splits a body.
pub(super) fn body_nodes(ss: &[Stmt]) -> usize {
    fn e_n(e: &Expr) -> usize {
        let mut n = 1;
        perry_hir::walker::walk_expr_children(e, &mut |c| n += e_n(c));
        n
    }
    fn s_n(s: &Stmt) -> usize {
        1 + match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_n(e),
            Stmt::Return(Some(e)) => e_n(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_n(condition)
                    + then_branch.iter().map(s_n).sum::<usize>()
                    + else_branch
                        .as_ref()
                        .map_or(0, |b| b.iter().map(s_n).sum::<usize>())
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_n(condition) + body.iter().map(s_n).sum::<usize>()
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(0, |i| s_n(i))
                    + condition.as_ref().map_or(0, e_n)
                    + update.as_ref().map_or(0, e_n)
                    + body.iter().map(s_n).sum::<usize>()
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_n(discriminant)
                    + cases
                        .iter()
                        .map(|c| {
                            c.test.as_ref().map_or(0, e_n) + c.body.iter().map(s_n).sum::<usize>()
                        })
                        .sum::<usize>()
            }
            _ => 0,
        }
    }
    ss.iter().map(s_n).sum()
}

/// Locals assigned (or declared) anywhere in `ss`, and in `extra`.
pub(super) fn assigned(ss: &[Stmt], extra: &[&Expr]) -> HashSet<u32> {
    fn e_walk(e: &Expr, out: &mut HashSet<u32>) {
        match e {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                out.insert(*id);
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    fn s_walk(s: &Stmt, out: &mut HashSet<u32>) {
        match s {
            Stmt::Let { id, init, .. } => {
                out.insert(*id);
                if let Some(e) = init {
                    e_walk(e, out);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => e_walk(e, out),
            Stmt::Return(Some(e)) => e_walk(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_walk(condition, out);
                then_branch.iter().for_each(|s| s_walk(s, out));
                if let Some(b) = else_branch {
                    b.iter().for_each(|s| s_walk(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_walk(condition, out);
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    s_walk(i, out);
                }
                if let Some(c) = condition {
                    e_walk(c, out);
                }
                if let Some(u) = update {
                    e_walk(u, out);
                }
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_walk(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        e_walk(t, out);
                    }
                    c.body.iter().for_each(|s| s_walk(s, out));
                }
            }
            _ => {}
        }
    }
    let mut out = HashSet::new();
    ss.iter().for_each(|s| s_walk(s, &mut out));
    extra.iter().for_each(|e| e_walk(e, &mut out));
    out
}

/// Static-key accesses in `ss` per candidate receiver, in first-seen key
/// order: `(receiver, key, is_store, expression identity, store value)`.
pub(super) fn accesses(ss: &[Stmt]) -> Vec<(Recv, String, bool, usize, Option<&Expr>)> {
    fn e_walk<'e>(e: &'e Expr, out: &mut Vec<(Recv, String, bool, usize, Option<&'e Expr>)>) {
        match e {
            Expr::PropertyGet {
                object, property, ..
            } => {
                if let Some(r) = Recv::of(object) {
                    out.push((r, property.clone(), false, e as *const Expr as usize, None));
                }
            }
            Expr::PutValueSet {
                target,
                key,
                receiver,
                value,
                ..
            } => {
                if let (Some(r), Expr::String(k)) = (Recv::of(target), key.as_ref()) {
                    if Recv::of(receiver) == Some(r) {
                        out.push((r, k.clone(), true, e as *const Expr as usize, Some(value)));
                    }
                }
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                if let Some(r) = Recv::of(object) {
                    out.push((
                        r,
                        property.clone(),
                        true,
                        e as *const Expr as usize,
                        Some(value),
                    ));
                }
            }
            _ => {}
        }
        // A method callee `o.m(...)` is not an own-slot read.
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                e_walk(object, out);
            } else {
                e_walk(callee, out);
            }
            for a in args {
                e_walk(a, out);
            }
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    fn s_walk<'e>(s: &'e Stmt, out: &mut Vec<(Recv, String, bool, usize, Option<&'e Expr>)>) {
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_walk(e, out),
            Stmt::Return(Some(e)) => e_walk(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_walk(condition, out);
                then_branch.iter().for_each(|s| s_walk(s, out));
                if let Some(b) = else_branch {
                    b.iter().for_each(|s| s_walk(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_walk(condition, out);
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    s_walk(i, out);
                }
                if let Some(c) = condition {
                    e_walk(c, out);
                }
                if let Some(u) = update {
                    e_walk(u, out);
                }
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_walk(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        e_walk(t, out);
                    }
                    c.body.iter().for_each(|s| s_walk(s, out));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    ss.iter().for_each(|s| s_walk(s, &mut out));
    out
}

/// Is `r` a binding whose value only this function's visible writes can
/// change, stored the plain way (a NaN-boxed root slot or a capture)?
pub(super) fn receiver_eligible(ctx: &FnCtx<'_>, r: Recv) -> bool {
    match r {
        // A derived constructor's `this` may still be in its TDZ (before
        // `super()`), and reading it throws: the preheader must not hoist
        // that read, so such a body's `this` is not a region receiver.
        //
        // A receiver whose shape is already PROVEN (Ptr<Shape>, or a
        // proven-shape method clone's `this`) stays eligible: the region adds
        // one guard but brings the R facts and bare stores a proof alone does
        // not (`h += o.a` on such a local: 24 -> 11 instructions), and its
        // reads and calls keep their proven routes inside the region
        // (`FnCtx::ptr_shape_receiver_fact`).
        Recv::This => {
            !ctx.this_stack.is_empty() && !ctx.in_static_member && ctx.super_called_stack.is_empty()
        }
        Recv::Local(id) => {
            !ctx.boxed_vars.contains(&id)
                && !ctx.prealloc_boxes.contains(&id)
                && !ctx.tdz_boxes.contains(&id)
                // A module-level binding can be assigned by any call, unless
                // it is `const`.
                && (!ctx.module_globals.contains_key(&id) || const_module_global(id))
                && !ctx.pod_records.contains_key(&id)
                && !ctx.scalar_replaced.contains_key(&id)
                && !ctx.spec_ta_bindings.contains_key(&id)
                && !ctx.local_slot_reps.contains_key(&id)
                && !ctx.integer_locals.contains(&id)
                && !ctx.receiver_descriptors.contains_buffer_view(id)
                && (ctx.locals.contains_key(&id)
                    || ctx.closure_captures.contains_key(&id)
                    || ctx.module_globals.contains_key(&id))
        }
    }
}

pub(super) struct Plan {
    /// `(receiver, keys, has a bare store, stored mask, boxed-store mask, R mask)`.
    pub(super) receivers: Vec<(Recv, Vec<String>, bool, u32, u32, u32)>,
    pub(super) bare: HashSet<usize>,
    pub(super) bare_reads: Vec<(usize, Recv, String)>,
    pub(super) number_local_uses: HashSet<u32>,
    /// All covered numeric operand reads, including ones a conservative
    /// first walk reached after a freshness kill.
    numeric_candidates: Vec<(usize, Recv, String)>,
    pub(super) declared_locals: HashSet<u32>,
    pub(super) trees: HashSet<usize>,
    pub(super) recheck: Recheck,
    /// Array receivers with a bare access, and what their accesses need.
    pub(super) arrays: Vec<(Recv, ArrayUse)>,
    /// The index expressions of bare VIEW accesses.
    pub(super) view_index: HashSet<usize>,
    /// Bare VIEW element reads (Numbers by construction) and every covered
    /// VIEW read (see `Planner::view_reads`).
    pub(super) view_reads: HashSet<usize>,
    view_cands: HashSet<usize>,
    /// Statements after which F-body sets the dirty flag (`Planner::mark_dirty`).
    pub(super) dirty_after: HashSet<usize>,
    /// The expressions the final walk judged able to run JS.
    pub(super) stale_at: HashSet<usize>,
}

/// What the top of an iteration must do before F-body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Recheck {
    /// Nothing between iterations can run JS.
    None,
    /// Only fact trees' generic arms can: re-check when the flag is set.
    Dirty,
    /// Every iteration.
    Always,
}

/// Plan the split of `tail` (the whole body for a loop region). `cands` are
/// the eligible receivers; the facts are FRESH at the tail's top. `loop_ctl`
/// carries the loop's condition and update for the re-check decision (loop
/// regions only).
/// Resolve exact R and 5L together by a descending fixed point. A first
/// conservative walk discovers covered numeric operand reads even after its
/// initial arithmetic kill. Propose those finite reads as R candidates, then
/// keep only ones the next walk really made bare and R-proven. The 5L local
/// set is recomputed after each reduction. A result is returned only when
/// both proofs agree with the same completed walk.
pub(super) fn plan(
    ctx: &FnCtx<'_>,
    tail: &[Stmt],
    cands: HashSet<Recv>,
    wide: &HashMap<Recv, String>,
    arrays: HashMap<Recv, ArrayUse>,
    env: &Env,
    loop_ctl: Option<(Option<&Expr>, Option<&Expr>)>,
    inner: Option<(usize, &HashSet<usize>, &HashSet<usize>, &HashSet<usize>)>,
) -> Option<Plan> {
    let empty_reads = HashSet::new();
    let empty_locals = HashSet::new();
    let seed = plan_once(
        ctx,
        tail,
        &cands,
        wide,
        &arrays,
        env,
        loop_ctl,
        inner,
        &empty_reads,
        &empty_locals,
    );
    let seed = seed?;
    let mut proof_reads: HashSet<usize> = seed
        .numeric_candidates
        .iter()
        .filter_map(|(ptr, recv, key)| {
            seed.receivers
                .iter()
                .find(|(r, keys, _, _, _, _)| r == recv && keys.contains(key))
                .map(|_| *ptr)
        })
        .collect();
    // VIEW reads start optimistic (every covered read), and the descending
    // fixed point keeps the ones a walk really makes bare.
    proof_reads.extend(seed.view_cands.iter().copied());
    if proof_reads.is_empty() {
        return Some(seed);
    }
    let loop_control: Vec<&Expr> = loop_ctl
        .map(|(cond, update)| cond.into_iter().chain(update).collect())
        .unwrap_or_default();
    let (locals, _) = number_facts_from_reads(
        ctx,
        tail,
        &loop_control,
        &proof_reads,
        &seed.number_local_uses,
        &seed.declared_locals,
    );
    let mut proof_locals: HashSet<u32> = locals.into_iter().collect();
    let limit = proof_reads.len() + proof_locals.len() + 1;
    for _ in 0..=limit {
        let p = plan_once(
            ctx,
            tail,
            &cands,
            wide,
            &arrays,
            env,
            loop_ctl,
            inner,
            &proof_reads,
            &proof_locals,
        )?;
        let reads: HashSet<usize> = p
            .bare_reads
            .iter()
            .filter_map(|(ptr, recv, key)| {
                p.receivers
                    .iter()
                    .find(|(r, _, _, _, _, _)| r == recv)
                    .and_then(|(_, keys, _, _, _, mask)| {
                        keys.iter()
                            .position(|k| k == key)
                            .filter(|i| mask & (1 << i) != 0)
                            .map(|_| *ptr)
                    })
            })
            .chain(p.view_reads.iter().copied())
            .collect();
        let (locals, _) = number_facts_from_reads(
            ctx,
            tail,
            &loop_control,
            &reads,
            &p.number_local_uses,
            &p.declared_locals,
        );
        let locals: HashSet<u32> = locals.into_iter().collect();
        if !reads.is_subset(&proof_reads) || !locals.is_subset(&proof_locals) {
            return None;
        }
        if reads == proof_reads && locals == proof_locals {
            return Some(p);
        }
        proof_reads = reads;
        proof_locals = locals;
    }
    None
}

fn plan_once<'p, 'a>(
    ctx: &'p FnCtx<'a>,
    tail: &[Stmt],
    cands: &'p HashSet<Recv>,
    wide: &HashMap<Recv, String>,
    arrays: &'p HashMap<Recv, ArrayUse>,
    env: &'p Env,
    loop_ctl: Option<(Option<&Expr>, Option<&Expr>)>,
    inner: Option<(
        usize,
        &'p HashSet<usize>,
        &'p HashSet<usize>,
        &'p HashSet<usize>,
    )>,
    proof_reads: &'p HashSet<usize>,
    proof_locals: &'p HashSet<u32>,
) -> Option<Plan> {
    if cands.is_empty() && arrays.is_empty() {
        return None;
    }
    let mut keys: HashMap<Recv, Vec<String>> = HashMap::new();
    let mut overflow: HashSet<Recv> = HashSet::new();
    for (r, k, _, _, _) in accesses(tail) {
        if !cands.contains(&r) {
            continue;
        }
        // A learned word addresses MAX_KEYS keys; a receiver whose class
        // the compiler names takes its slots from the static birth shape
        // (checked against the keys at the end of the plan).
        let limit = if wide.contains_key(&r) { 31 } else { MAX_KEYS };
        let list = keys.entry(r).or_default();
        if !list.contains(&k) {
            if list.len() == limit {
                overflow.insert(r);
            } else {
                list.push(k);
            }
        }
    }
    let cands: HashSet<Recv> = cands
        .iter()
        .copied()
        .filter(|r| keys.contains_key(r) && !overflow.contains(r))
        .collect();
    if cands.is_empty() && arrays.is_empty() {
        return None;
    }
    let fresh: BTreeMap<Recv, u8> = cands
        .iter()
        .chain(arrays.keys())
        .map(|r| (*r, FRESH))
        .collect();
    let mut p = Planner {
        ctx,
        cands: &cands,
        keys: &keys,
        arrays: &arrays,
        env,
        bare: HashSet::new(),
        number_reads: Vec::new(),
        number_local_uses: HashSet::new(),
        bare_reads: Vec::new(),
        bare_arrays: HashSet::new(),
        view_index: HashSet::new(),
        view_reads: Vec::new(),
        view_cands: Vec::new(),
        inner: inner.map(|(_, b, t, s)| (b, t, s)),
        in_inner: false,
        record_dirty: false,
        in_store_rhs: false,
        trees: HashSet::new(),
        bare_stores: HashSet::new(),
        boxed_stores: HashMap::new(),
        continues: Vec::new(),
        record: true,
        proof_reads,
        proof_locals,
        // Loop regions with array receivers. A statement in a nested body
        // region's tail sets the flag from that region's F-tail (its
        // `dirty_slot` is the loop's).
        mark_dirty: loop_ctl.is_some() && !arrays.is_empty(),
        dirty_after: HashSet::new(),
        stale_at: std::cell::RefCell::new(HashSet::new()),
    };
    let end = match inner {
        // The nested body region's tail: F-tail keeps the loop's facts
        // (its fact trees set the dirty flag), G-tail leaves the loop region.
        // Loop accesses inside it are not recorded (the tail lowers under
        // the body region's facts).
        Some((k, _, _, _)) => {
            let st = p.stmts(&tail[..k], Some(fresh.clone()));
            p.record = false;
            p.in_inner = true;
            p.record_dirty = true;
            let st = p.stmts(&tail[k..], st);
            p.record_dirty = false;
            p.in_inner = false;
            p.record = true;
            st
        }
        None => p.stmts(tail, Some(fresh.clone())),
    };
    let conts = std::mem::take(&mut p.continues);
    let mut recheck = Recheck::None;
    if let Some((cond, update)) = loop_ctl {
        p.record = false;
        let mut back = end;
        for c in conts {
            back = meet(back, c);
        }
        if let Some(u) = update {
            back = p.expr(u, back);
        }
        // Entry from the preheader and from the back edge both evaluate the
        // condition before the body.
        let mut head = meet(Some(fresh.clone()), back);
        if let Some(c) = cond {
            head = p.expr(c, head);
        }
        recheck = match &head {
            None => Recheck::None,
            Some(m) => {
                let worst = cands
                    .iter()
                    .chain(arrays.keys())
                    .map(|r| m.get(r).copied().unwrap_or(0))
                    .min()
                    .unwrap_or(FRESH);
                if worst == FRESH {
                    Recheck::None
                } else if worst == DIRTY {
                    Recheck::Dirty
                } else {
                    Recheck::Always
                }
            }
        };
    }
    let bare = std::mem::take(&mut p.bare);
    let view_index = std::mem::take(&mut p.view_index);
    let view_reads: HashSet<usize> = std::mem::take(&mut p.view_reads).into_iter().collect();
    let view_cands: HashSet<usize> = std::mem::take(&mut p.view_cands).into_iter().collect();
    let number_reads = std::mem::take(&mut p.number_reads);
    let bare_reads = std::mem::take(&mut p.bare_reads);
    let mut number_local_uses = std::mem::take(&mut p.number_local_uses);
    let (flow_reads, flow_locals, declared_locals) =
        crate::collectors::region_number_flow_reads(tail, &number_local_uses);
    number_local_uses.extend(flow_locals);
    let trees = std::mem::take(&mut p.trees);
    let dirty_after = std::mem::take(&mut p.dirty_after);
    let bare_stores = std::mem::take(&mut p.bare_stores);
    let boxed_stores = std::mem::take(&mut p.boxed_stores);
    let mut plan_arrays: Vec<(Recv, ArrayUse)> = std::mem::take(&mut p.bare_arrays)
        .into_iter()
        .map(|r| (r, arrays[&r]))
        .collect();
    plan_arrays.sort();
    if bare.is_empty() {
        return None;
    }
    // Only receivers with a bare access need a guard.
    let used: HashSet<Recv> = {
        let mut u = HashSet::new();
        for (r, k, _, _, _) in accesses(tail) {
            if cands.contains(&r) && keys[&r].contains(&k) {
                u.insert(r);
            }
        }
        u
    };
    // Which of each receiver's keys the body ever STORES (bare or not): the
    // runtime serves a spill-located key to reads only, so a stored key must
    // be inline for the word to be published.
    let mut stored: HashMap<Recv, u32> = HashMap::new();
    for (r, k, is_store, _, _) in accesses(tail) {
        if is_store && cands.contains(&r) {
            if let Some(i) = keys[&r].iter().position(|x| *x == k) {
                *stored.entry(r).or_default() |= 1 << i;
            }
        }
    }
    // A store RHS may use loop-carried locals discharged by the same 5L
    // entry/re-entry checks as its reads. Reuse that exact set when judging
    // whether its value is compatible with an F64 lane.
    let mut numeric_locals: HashSet<u32> =
        ctx.number_by_construction_locals.iter().copied().collect();
    numeric_locals.extend(proof_locals.iter().copied());
    let mut receivers: Vec<(Recv, Vec<String>, bool, u32, u32, u32)> = used
        .into_iter()
        .map(|r| {
            let boxed = boxed_stores.get(&r).map_or(0, |ks| {
                keys[&r]
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| ks.contains(*k))
                    .fold(0u32, |m, (i, _)| m | 1 << i)
            });
            let r_mask = bare_reads.iter().fold(0u32, |mask, (ptr, nr, key)| {
                if *nr == r
                    && (flow_reads.contains(ptr) || number_reads.iter().any(|(p, _, _)| p == ptr))
                {
                    if let Some(i) = keys[&r].iter().position(|k| k == key) {
                        return mask | (1 << i);
                    }
                }
                mask
            });
            // The first planner walk has no R facts, so it conservatively
            // marks `o.x = o.x + 1` boxed. Rejudge only its exact bare stores
            // against the R that this plan will require at F entry. All
            // stores to a key must prove Number before its boxed bit clears.
            let f64_reads: HashSet<usize> = bare_reads
                .iter()
                .filter(|(_, nr, key)| {
                    *nr == r
                        && keys[&r]
                            .iter()
                            .position(|k| k == key)
                            .is_some_and(|i| r_mask & (1 << i) != 0)
                })
                .map(|(ptr, _, _)| *ptr)
                .collect();
            let mut boxed = boxed;
            for (i, key) in keys[&r].iter().enumerate() {
                if boxed & (1 << i) == 0 {
                    continue;
                }
                let stores: Vec<&Expr> = accesses(tail)
                    .into_iter()
                    .filter(|(sr, sk, is_store, ptr, _)| {
                        *sr == r && sk == key && *is_store && bare.contains(ptr)
                    })
                    .filter_map(|(_, _, _, _, value)| value)
                    .collect();
                if !stores.is_empty()
                    && stores.iter().all(|value| {
                        crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value)
                            || crate::collectors::region_store_value_is_number(
                                value,
                                &f64_reads,
                                &numeric_locals,
                                ctx.not_bigint_locals,
                            )
                    })
                {
                    boxed &= !(1 << i);
                }
            }
            (
                r,
                keys[&r].clone(),
                bare_stores.contains(&r),
                stored.get(&r).copied().unwrap_or(0),
                boxed,
                r_mask,
            )
        })
        .collect();
    receivers.sort_by_key(|(r, _, _, _, _, _)| *r);
    // R reads reached through local value flow need the same freshness/5L
    // rewalk as direct arithmetic leaves. Otherwise lowering sees their R
    // mask and emits a numeric F body while planning keeps the seed recheck.
    let numeric_candidates = number_reads
        .into_iter()
        .chain(
            bare_reads
                .iter()
                .filter(|(ptr, _, _)| flow_reads.contains(ptr))
                .cloned(),
        )
        .collect();
    // A receiver wider than a learned word is served only by its static
    // supplier: every key it names, with the plan's boxed stores.
    for (r, k, _, _, bm, _) in &receivers {
        if k.len() > MAX_KEYS
            && !wide
                .get(r)
                .is_some_and(|c| super::guard::static_keys_served(ctx, c, k, *bm))
        {
            return None;
        }
    }
    Some(Plan {
        receivers,
        bare,
        bare_reads,
        number_local_uses,
        numeric_candidates,
        declared_locals,
        trees,
        recheck,
        arrays: plan_arrays,
        view_index,
        view_reads,
        view_cands,
        dirty_after,
        stale_at: p.stale_at.take(),
    })
}
