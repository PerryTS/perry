//! Representation-selection Phase 3b, **#7034 §3: array-element shape facts**.
//!
//! ## The escape this opens, and why it is not the return escape
//!
//! `collectors/ptr_shape.rs` rule 2 disqualifies a local at any escape.
//! #7034 §4 opened the `return` position; this file opens
//! `array/object element` for the array half — `rows.push(row)` /
//! `for (const r of rows) r.field`. Arrays of records are the data model of
//! real code, and #7139 measured the cost of not having it: of the ~103
//! `Ptr<Shape>` candidates its CommonJS barrier exemption freed in real
//! dependency modules, **100 were immediately re-denied by rule 2**.
//!
//! `return` was easy because a return is a **terminator**: no use of the local
//! can follow it, so every access the pass licenses already ran while the
//! object was still unaliased. An element escape is not a terminator — the
//! object stays reachable through the array for the rest of its life, and a
//! write through `arr[i]` to a non-declared field would invalidate the shape
//! proof after the fact. So the containment region has to be widened from
//! *one local* to *one local array and everything derived from it*, and the
//! array's own uses have to be bounded exactly as an object local's are.
//!
//! ## The rule
//!
//! A region-local binding `A` is an **element-shape-proven array** of class
//! `C` when every one of these holds. Each is an independent conjunct with its
//! own red test in `ptr_shape_elements_tests.rs`.
//!
//! * **E1 — array provenance.** Exactly one `Stmt::Let { mutable: false,
//!   init: Some(Expr::Array([])) }` binds `A`, and `A` is neither boxed nor a
//!   module global. The literal must be **empty**: a non-empty literal can
//!   carry elisions (`[,,]` — holes that read back as `undefined`), and
//!   admitting one buys nothing that the pushes below do not.
//! * **E2 — element provenance.** Every write into `A` is
//!   `Expr::ArrayPush { array_id: A }` whose value is `new C(...)` — inline,
//!   or a local bound by exactly one `Let { init: New { C } }` and pushed
//!   exactly once. `Expr::New` covers closed object literals too
//!   (`__AnonShape_…`), so records qualify. Perry class constructors cannot
//!   return an override object, so the dynamic class is *exactly* `C`. No
//!   other mutator is admitted at all — not `pop`/`shift`/`splice`/`unshift`/
//!   `copyWithin`, not `IndexSet`, not a `length` write — which is what makes
//!   `A` **dense** and monomorphic for its whole lifetime.
//! * **E3 — array containment.** Every *other* use of `A` is an in-bounds
//!   element read (E5), a `.length` read, or `return A`. The return exemption
//!   is #7034 §4's, unchanged and for the same reason. Anything else — call
//!   argument, closure capture, reassignment, `IndexSet`, an unrecognised
//!   array method, being an element of another container — disqualifies `A`.
//! * **E4 — class admissibility.** `C` passes the same `chain_admissible`
//!   gate rule 1 applies to a `new C(...)` local, and the module-wide rule-5
//!   barrier scan is clear.
//! * **E5 — in-bounds reads.** An element read is licensed only at
//!   `A[i]` where `i` is the induction variable of an enclosing
//!   `for (let i = 0; i < A.length; i++)` — literally that shape: zero init,
//!   `Lt` against `A.length`, `++` update, and `i` written nowhere else in
//!   the region. Because E3 admits no mutator that can *shrink* `A`, and
//!   because `A` is dense by E2, `0 <= i < A.length` at the read means
//!   `A[i]` is an own element, hence an instance of `C`. **This conjunct is
//!   the whole difference between this pass and a wrong one**: without it
//!   `A[i]` can be `undefined`, and a guard-free fixed-offset load masks a
//!   NaN-boxed `undefined` into a wild pointer.
//!
//! `for (const r of A)` desugars to exactly the E5 shape
//! (`lower/stmt_loops.rs::lazy_or_index_elem` — a `__idx` local, `__idx <
//! __arr.length`, `Let r = IndexGet(__arr, __idx)`), so the iterator form is
//! covered by the indexed proof rather than by a second one.
//!
//! ## What the facts are used for
//!
//! Two halves, both consumed in `ptr_shape.rs`:
//!
//! 1. **Producer side.** `rows.push(row)` stops disqualifying `row`
//!    ([`ElementShapeFacts::push_is_contained`]).
//! 2. **Reader side.** `const r = A[i]` at a licensed site is rule-1
//!    provenance of `new C(...)` strength
//!    ([`ElementShapeFacts::element_read_class`]), so `r` becomes an ordinary
//!    Phase 3b candidate and every `r.field` in the loop body lowers
//!    guard-free through the machinery that already exists.
//!
//! ## Group integrity — why one failure voids the whole array
//!
//! Every member of an element group (the pushed producers plus the
//! element-read locals) is a reference to an object that the *other* members
//! also reach. If any one of them fails rule 2 — `r.extra = 1` adds a
//! property, a closure captures it, it is passed to an opaque call — the
//! objects in `A` can transition shape, and every other member's guard-free
//! access is then wrong. `ptr_shape.rs` therefore drops the **entire group**
//! when any member fails, rather than dropping the member. See
//! [`ElementShapeFacts::group_members`].
//!
//! ## `numeric_fields` is deliberately not claimed
//!
//! Phase 3b's numeric-field proof is an *exhaustive reachable store* proof,
//! which containment makes possible because no alias exists. An element group
//! has aliases by construction: a store through one member
//! (`r.score = "s"` — a declared field, so rule 2 permits it) takes the
//! store-side boxed-setter exit and downgrades that slot's raw-f64 layout,
//! and another member's `load double` claiming `JsNumber` would then read
//! NaN-boxed string bits as a number. Group members therefore claim no
//! numeric fields at all — the same stand-down `proven_this.rs` and
//! `ptr_shape_returns.rs` make, for the same reason. The shape proof by
//! itself still retires the whole guard diamond.
//!
//! ## GC contract
//!
//! **No new site holds an object pointer at rest.** An element-read local is
//! an ordinary NaN-boxed slot, shadow-bound by `collect_pointer_typed_locals`
//! / `js_shadow_slot_bind` like any other object local, and every access
//! re-derives the raw pointer from that slot inside one region. `TaPtr`'s
//! callee-side no-bind shortcut is NOT copied — it is sound only for
//! non-movable typed-array storage, and `GC_TYPE_OBJECT` is movable (#6990,
//! #7019).
//!
//! That rooting is a **proof obligation, not an assumption**:
//! `collect_pointer_typed_locals` decides a local's slot from its init
//! expression's inferred type, and `IndexGet` on a local typed
//! `Array(Number)` infers `Number` — no slot. A lying annotation would leave a
//! promoted `Ptr<Shape>` element in an unrooted alloca and an evacuating minor
//! would move the object without rewriting it. Both the array's declared
//! element type and the element local's own declared type are therefore
//! checked against `pointer_locals::is_definitely_non_pointer_type` before any
//! fact is issued.
//!
//! Write barriers on the `push` are untouched: a proven shape says nothing
//! about whether the stored value is a pointer, and this pass changes no store
//! lowering at all.
//!
//! Gated by `PERRY_PTR_SHAPE_LOCALS` along with the rest of Phase 3b — no new
//! env knob, so there is no new unexercised off-state (CLAUDE.md's GC knob
//! kill-policy).

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Class, Expr, Stmt};

use super::pointer_locals::is_definitely_non_pointer_type;
use super::ptr_shape::{chain_admissible, ptr_shape_locals_enabled};
use super::ModuleDispatchFacts;

/// Element-shape facts for one lowered region.
#[derive(Debug, Default, Clone)]
pub(crate) struct ElementShapeFacts {
    /// Array ROOT id -> proven element class.
    arrays: HashMap<u32, String>,
    /// Array alias id (including the root itself) -> root id.
    array_roots: HashMap<u32, u32>,
    /// Producer local -> (array root it is pushed into, element class).
    pushed: HashMap<u32, (u32, String)>,
    /// Element-read local -> (array root it was read from, element class).
    element_reads: HashMap<u32, (u32, String)>,
}

impl ElementShapeFacts {
    pub(crate) fn is_empty(&self) -> bool {
        self.arrays.is_empty()
    }

    /// Test-only: are ALL fact maps empty? `is_empty` gates every consumer and
    /// keys on `arrays` alone; the other three are kept consistent with it by
    /// construction, and `tests::is_empty_covers_every_fact_map` is what holds
    /// that invariant in place.
    #[cfg(test)]
    pub(crate) fn debug_all_maps_empty(&self) -> bool {
        self.arrays.is_empty()
            && self.array_roots.is_empty()
            && self.pushed.is_empty()
            && self.element_reads.is_empty()
    }

    /// Is `value_local`'s push into `array_local` covered by a proven array,
    /// so rule 2 may treat that element position as contained?
    pub(crate) fn push_is_contained(&self, value_local: u32, array_local: u32) -> bool {
        let Some(root) = self.array_roots.get(&array_local) else {
            return false;
        };
        matches!(self.pushed.get(&value_local), Some((r, _)) if r == root)
    }

    /// The proven element class of a licensed `const r = A[i]` binding.
    pub(crate) fn element_read_class(&self, local: u32) -> Option<&str> {
        self.element_reads.get(&local).map(|(_, c)| c.as_str())
    }

    /// Every local that references an object stored in `root`'s array.
    ///
    /// Group integrity: `ptr_shape.rs` promotes all of these or none of them.
    pub(crate) fn group_members(&self) -> HashMap<u32, Vec<u32>> {
        let mut out: HashMap<u32, Vec<u32>> = HashMap::new();
        for (id, (root, _)) in self.pushed.iter().chain(self.element_reads.iter()) {
            out.entry(*root).or_default().push(*id);
        }
        out
    }

    /// Locals whose object is reachable from an array, and which therefore
    /// must not claim numeric fields (module doc).
    pub(crate) fn is_group_member(&self, local: u32) -> bool {
        self.pushed.contains_key(&local) || self.element_reads.contains_key(&local)
    }
}

/// Entry point: prove the element-shape-proven local arrays of one region.
///
/// Purely syntactic — it never re-enters
/// [`super::ptr_shape::collect_shape_proven_ptr_locals`], so there is no
/// recursion between the two passes and no fixpoint to converge. Rule 2
/// containment of the resulting group members is enforced by that pass
/// afterwards, via the group-integrity filter.
pub(crate) fn collect_element_shape_facts(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
) -> ElementShapeFacts {
    let out = ElementShapeFacts::default();
    if !ptr_shape_locals_enabled() || module_dispatch.has_shape_barrier_sites() {
        return out;
    }

    // E1: array provenance.
    let mut let_counts: HashMap<u32, u32> = HashMap::new();
    let mut array_seeds: HashMap<u32, Type> = HashMap::new();
    let mut new_lets: HashMap<u32, Option<String>> = HashMap::new();
    let mut elem_let_ty: HashMap<u32, Type> = HashMap::new();
    walk_stmts(stmts, &mut |s| {
        let Stmt::Let {
            id,
            ty,
            mutable,
            init,
            ..
        } = s
        else {
            return;
        };
        *let_counts.entry(*id).or_insert(0) += 1;
        elem_let_ty.insert(*id, ty.clone());
        match init.as_ref() {
            Some(Expr::Array(items)) if !*mutable && items.is_empty() => {
                if !boxed_vars.contains(id) && !module_globals.contains_key(id) {
                    array_seeds.insert(*id, ty.clone());
                }
            }
            Some(Expr::New { class_name, .. }) => {
                // Producer-local provenance for E2's `LocalGet` form. A second
                // `Let` for the same id poisons it via `let_counts`.
                if !boxed_vars.contains(id) && !module_globals.contains_key(id) {
                    new_lets.insert(*id, Some(class_name.clone()));
                }
            }
            _ => {}
        }
    });
    array_seeds.retain(|id, _| let_counts.get(id).copied().unwrap_or(0) == 1);
    new_lets.retain(|id, _| let_counts.get(id).copied().unwrap_or(0) == 1);
    if array_seeds.is_empty() {
        return out;
    }

    // Array aliases: `const b = a` tracks the same array object. Same shape
    // (and same reason) as `ptr_shape.rs`'s alias pre-pass — the `for…of`
    // desugar binds `const __arr_N = <the iterable>` before the loop.
    let mut array_roots: HashMap<u32, u32> = array_seeds.keys().map(|id| (*id, *id)).collect();
    let mut alias_edges: Vec<(u32, u32)> = Vec::new();
    super::ptr_shape::collect_alias_edges(stmts, &mut alias_edges);
    loop {
        let mut changed = false;
        for (alias, src) in &alias_edges {
            if array_roots.contains_key(alias)
                || boxed_vars.contains(alias)
                || module_globals.contains_key(alias)
                || let_counts.get(alias).copied().unwrap_or(0) != 1
            {
                continue;
            }
            if let Some(&root) = array_roots.get(src) {
                array_roots.insert(*alias, root);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // E3/E5: the array use walk.
    let mut walk = ArrayWalk {
        roots: &array_roots,
        disqualified: HashSet::new(),
        pushes: HashMap::new(),
        reads: Vec::new(),
        idx_writes: HashMap::new(),
        bounded: Vec::new(),
        in_closure: false,
    };
    walk.walk_stmts(stmts);
    let ArrayWalk {
        mut disqualified,
        pushes,
        reads,
        idx_writes,
        ..
    } = walk;

    // E2: one element class per array, from the push sites.
    let mut arrays: HashMap<u32, String> = HashMap::new();
    let mut pushed: HashMap<u32, (u32, String)> = HashMap::new();
    // A producer local pushed more than once is not exempted: two element
    // groups would both claim it and the single-group soundness argument
    // ("every reference to this object is a member of this group") no longer
    // holds. Cheap to count, and it keeps the argument one sentence long.
    let mut push_value_counts: HashMap<u32, u32> = HashMap::new();
    for sites in pushes.values() {
        for site in sites {
            if let PushValue::Local(v) = site {
                *push_value_counts.entry(*v).or_insert(0) += 1;
            }
        }
    }
    for (root, sites) in &pushes {
        if disqualified.contains(root) || sites.is_empty() {
            continue;
        }
        let mut class_name: Option<&str> = None;
        let mut ok = true;
        let mut members: Vec<u32> = Vec::new();
        for site in sites {
            let name = match site {
                PushValue::Fresh(c) => c.as_str(),
                PushValue::Local(v) => {
                    if push_value_counts.get(v).copied().unwrap_or(0) != 1 {
                        ok = false;
                        break;
                    }
                    match new_lets.get(v) {
                        Some(Some(c)) => {
                            members.push(*v);
                            c.as_str()
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                PushValue::Other => {
                    ok = false;
                    break;
                }
            };
            match class_name {
                None => class_name = Some(name),
                Some(prev) if prev == name => {}
                Some(_) => {
                    ok = false;
                    break;
                }
            }
        }
        let Some(class_name) = class_name.filter(|_| ok) else {
            disqualified.insert(*root);
            continue;
        };
        // E4 + the GC rooting obligation on the array's declared element type.
        if !classes.contains_key(class_name)
            || !chain_admissible(classes, class_name)
            || !array_type_keeps_element_slot(array_seeds.get(root))
        {
            disqualified.insert(*root);
            continue;
        }
        arrays.insert(*root, class_name.to_string());
        for m in members {
            pushed.insert(m, (*root, class_name.to_string()));
        }
    }
    if arrays.is_empty() {
        return ElementShapeFacts::default();
    }
    pushed.retain(|_, (root, _)| arrays.contains_key(root));

    // E5: element-read seeds at licensed sites.
    let mut element_reads: HashMap<u32, (u32, String)> = HashMap::new();
    for read in &reads {
        let Some(class_name) = arrays.get(&read.root) else {
            continue;
        };
        // The induction variable must be written exactly twice in the whole
        // region — its `Let` and its `++` — and by the loop that bounds it.
        if idx_writes.get(&read.index).copied().unwrap_or(0) != 2
            || boxed_vars.contains(&read.index)
            || module_globals.contains_key(&read.index)
        {
            continue;
        }
        if boxed_vars.contains(&read.local) || module_globals.contains_key(&read.local) {
            continue;
        }
        if let_counts.get(&read.local).copied().unwrap_or(0) != 1 {
            continue;
        }
        // GC: the binding must keep a shadow slot (module doc).
        if elem_let_ty
            .get(&read.local)
            .is_some_and(is_definitely_non_pointer_type)
        {
            continue;
        }
        element_reads.insert(read.local, (read.root, class_name.clone()));
    }

    ElementShapeFacts {
        arrays,
        array_roots,
        pushed,
        element_reads,
    }
}

/// Would an `A[i]` binding keep its shadow-stack root slot?
///
/// `collect_pointer_typed_locals` infers an `IndexGet`'s value type from the
/// object's type: `Array(elem)` yields `elem`, and a `Number`/`Boolean`/…
/// element type means "definitely not a pointer", which drops the slot. A
/// declared `number[]` that this pass proved holds `C` instances (Perry does
/// not validate annotations) would then leave a promoted element unrooted —
/// #7019 ships an evacuating minor by default, so that is a live wrong answer,
/// not a theoretical one. Refuse the fact instead of trusting the annotation.
fn array_type_keeps_element_slot(ty: Option<&Type>) -> bool {
    match ty {
        Some(Type::Array(elem)) => !is_definitely_non_pointer_type(elem),
        // Any other declared type either yields no `IndexGet` inference (so
        // the binding's own declared type decides, checked separately at the
        // read site) or is not an array type at all.
        _ => true,
    }
}

// ── The array use walk ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum PushValue {
    /// `A.push(new C(…))` — inline allocation.
    Fresh(String),
    /// `A.push(v)` — a local.
    Local(u32),
    /// Anything else; disqualifies the array.
    Other,
}

struct ReadSite {
    /// The array root read from.
    root: u32,
    /// The induction variable local.
    index: u32,
    /// The local the element was bound to.
    local: u32,
}

struct ArrayWalk<'a> {
    roots: &'a HashMap<u32, u32>,
    disqualified: HashSet<u32>,
    pushes: HashMap<u32, Vec<PushValue>>,
    reads: Vec<ReadSite>,
    /// local id -> number of writes (`Let` / `LocalSet` / `Update`) anywhere
    /// in the region, closures included.
    idx_writes: HashMap<u32, u32>,
    /// Induction variables currently proven `0 <= i < root.length`, innermost
    /// last.
    bounded: Vec<(u32, u32)>,
    in_closure: bool,
}

impl<'a> ArrayWalk<'a> {
    fn root_of(&self, id: u32) -> Option<u32> {
        self.roots.get(&id).copied()
    }

    fn disq(&mut self, id: u32) {
        if let Some(root) = self.root_of(id) {
            self.disqualified.insert(root);
        }
    }

    fn note_write(&mut self, id: u32) {
        *self.idx_writes.entry(id).or_insert(0) += 1;
    }

    fn walk_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                self.note_write(*id);
                if let Some(e) = init {
                    // An array alias binding is the tracked edge itself.
                    if let Expr::LocalGet(src) = e {
                        if self.root_of(*src).is_some() && self.root_of(*id).is_some() {
                            return;
                        }
                    }
                    // E5: `const r = A[i]` at a licensed site. This is the
                    // ONLY position an element read is admitted in — see the
                    // `Expr::IndexGet` arm for why every other one is an
                    // escape of the ELEMENT (as opposed to the array).
                    if let Expr::IndexGet { object, index } = e {
                        if let (Expr::LocalGet(a), Expr::LocalGet(i)) =
                            (object.as_ref(), index.as_ref())
                        {
                            if let Some(root) = self.root_of(*a) {
                                if !self.in_closure
                                    && self.bounded.iter().any(|(bi, br)| bi == i && *br == root)
                                {
                                    self.reads.push(ReadSite {
                                        root,
                                        index: *i,
                                        local: *id,
                                    });
                                    return;
                                }
                            }
                        }
                    }
                    self.walk_expr(e);
                }
            }
            Stmt::Return(Some(e)) => {
                // #7034 §4's terminator exemption, applied to the array.
                if !self.in_closure {
                    if let Expr::LocalGet(id) = e {
                        if self.root_of(*id).is_some() {
                            return;
                        }
                    }
                }
                self.walk_expr(e);
            }
            Stmt::Return(None) => {}
            Stmt::Expr(e) => self.walk_expr(e),
            Stmt::Throw(e) => self.walk_expr(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition);
                self.walk_stmts(then_branch);
                if let Some(eb) = else_branch {
                    self.walk_stmts(eb);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.walk_expr(condition);
                self.walk_stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                let bound = self.bounded_induction(init.as_deref(), condition, update);
                if let Some(init) = init {
                    self.walk_stmt(init.as_ref());
                }
                if let Some(c) = condition {
                    self.walk_expr(c);
                }
                if let Some(u) = update {
                    self.walk_expr(u);
                }
                let pushed_bound = bound.is_some();
                if let Some(b) = bound {
                    self.bounded.push(b);
                }
                self.walk_stmts(body);
                if pushed_bound {
                    self.bounded.pop();
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_stmts(body);
                if let Some(c) = catch {
                    self.walk_stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.walk_stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.walk_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.walk_expr(t);
                    }
                    self.walk_stmts(&case.body);
                }
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(body.as_ref()),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    /// E5: recognise `for (let i = 0; i < A.length; i++)` and return
    /// `(i, root)`.
    fn bounded_induction(
        &self,
        init: Option<&Stmt>,
        condition: &Option<Expr>,
        update: &Option<Expr>,
    ) -> Option<(u32, u32)> {
        if self.in_closure {
            return None;
        }
        let Some(Stmt::Let {
            id: idx,
            mutable: true,
            init: Some(zero),
            ..
        }) = init
        else {
            return None;
        };
        match zero {
            Expr::Number(n) if *n == 0.0 => {}
            Expr::Integer(0) => {}
            _ => return None,
        }
        let Some(Expr::Compare {
            op: perry_hir::CompareOp::Lt,
            left,
            right,
        }) = condition
        else {
            return None;
        };
        if !matches!(left.as_ref(), Expr::LocalGet(l) if l == idx) {
            return None;
        }
        let Expr::PropertyGet {
            object, property, ..
        } = right.as_ref()
        else {
            return None;
        };
        if property != "length" {
            return None;
        }
        let Expr::LocalGet(a) = object.as_ref() else {
            return None;
        };
        let root = self.root_of(*a)?;
        match update {
            Some(Expr::Update {
                id,
                op: perry_hir::UpdateOp::Increment,
                ..
            }) if id == idx => {}
            _ => return None,
        }
        Some((*idx, root))
    }

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            // `A.length` — the only property read admitted on the array.
            Expr::PropertyGet {
                object, property, ..
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.root_of(*id).is_some() {
                        if property != "length" {
                            self.disq(*id);
                        }
                        return;
                    }
                }
                self.walk_expr(object);
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.root_of(*id).is_some() {
                        // Any property write on the array itself — including
                        // `A.length = 0`, which would punch holes.
                        self.disq(*id);
                        self.walk_expr(value);
                        return;
                    }
                }
                let _ = property;
                self.walk_expr(object);
                self.walk_expr(value);
            }
            Expr::PropertyUpdate { object, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.root_of(*id).is_some() {
                        self.disq(*id);
                        return;
                    }
                }
                self.walk_expr(object);
            }
            // E2: the one admitted write.
            Expr::ArrayPush { array_id, value } => {
                if let Some(root) = self.root_of(*array_id) {
                    let site = match value.as_ref() {
                        Expr::New { class_name, .. } => PushValue::Fresh(class_name.clone()),
                        Expr::LocalGet(v) if self.root_of(*v).is_none() => PushValue::Local(*v),
                        _ => PushValue::Other,
                    };
                    self.pushes.entry(root).or_default().push(site);
                    // A tracked array stored as an ELEMENT of another array is
                    // reachable and mutable through that array —
                    // `outer[0][0] = x` is an `IndexSet` on an `IndexGet`,
                    // which neither walk tracks. Making the outer array
                    // `PushValue::Other` disqualifies the OUTER one only, and
                    // the arm below deliberately skips `walk_expr` for a
                    // `LocalGet` value, so the inner one would never reach the
                    // bare-reference arm. Disqualify it here. A producer local
                    // is not a tracked root, so the admitted `PushValue::Local`
                    // case is unaffected. (CodeRabbit, PR #7149.)
                    if let Expr::LocalGet(v) = value.as_ref() {
                        self.disq(*v);
                        return;
                    }
                    // Still walk for OTHER arrays nested in the value.
                    self.walk_expr(value);
                    return;
                }
                self.walk_expr(value);
            }
            // Every other id-keyed container mutator is a hard disqualifier.
            Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                self.disq(*array_id);
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => self.disq(*id),
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.disq(*id);
                }
                self.walk_expr(object);
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Expr::IndexUpdate { object, index, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.disq(*id);
                }
                self.walk_expr(object);
                self.walk_expr(index);
            }
            // ★ An element read the `Stmt::Let` arm did not license.
            //
            // It is tempting to treat this as harmless — a read cannot
            // transition a shape — but it hands out a REFERENCE to an element
            // that this walk then stops tracking. `f(A[i])` lets an opaque
            // callee add a property to an object that a licensed
            // `const s = A[k]` reads guard-free; `A[i].m()` runs a method
            // whose `this`-flow nothing vetted; `const r = A[0]` (a literal
            // index, so unlicensed) binds an element to a local that rule 2
            // never sees, and `r.extra = 1` reshapes it. All three would make
            // the OTHER members' fixed-offset loads wrong.
            //
            // So every unlicensed element read disqualifies the array. Direct
            // `A[i].field` access is therefore not covered at all today, and
            // widening to it needs the element class in this walk (which is
            // only known after the push scan) — tracked in #7151.
            Expr::IndexGet { object, index } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.disq(*id);
                }
                self.walk_expr(object);
                self.walk_expr(index);
            }
            Expr::LocalSet(id, v) => {
                self.note_write(*id);
                self.disq(*id);
                self.walk_expr(v);
            }
            Expr::Update { id, .. } => {
                self.note_write(*id);
                self.disq(*id);
            }
            // Any bare reference to the array in a position the arms above did
            // not admit — a call argument, a `new` argument, an element of
            // another container, a spread, an untracked `Let` init (mutable,
            // boxed, or module-global, so the alias pre-pass skipped it) —
            // creates an alias this walk cannot follow.
            Expr::LocalGet(id) => self.disq(*id),
            Expr::Closure {
                body,
                captures,
                mutable_captures,
                ..
            } => {
                for c in captures.iter().chain(mutable_captures.iter()) {
                    self.disq(*c);
                }
                let outer = self.in_closure;
                self.in_closure = true;
                self.walk_stmts(body);
                self.in_closure = outer;
            }
            _ => {
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
        }
    }
}

/// Statement walker over one region's statement tree.
///
/// It does NOT descend into closure bodies — those live inside `Expr`s, not
/// `Stmt`s. That is correct for both callers and deliberately so:
/// `collect_element_shape_facts` uses it for the E1 seed scan, where a closure
/// body's locals are its own ids and an outer array referenced from one is
/// disqualified by `ArrayWalk` (which does descend, through its own
/// `Expr::Closure` arm); and `element_read_seeds` uses it for the read seeds,
/// where `bounded_induction` already refuses to license anything under
/// `in_closure`, so a closure-body read is never a seed to find.
fn walk_stmts<'a>(stmts: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk_stmts(then_branch, f);
                if let Some(eb) = else_branch {
                    walk_stmts(eb, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk_stmts(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    walk_stmts(std::slice::from_ref(init.as_ref()), f);
                }
                walk_stmts(body, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, f);
                if let Some(c) = catch {
                    walk_stmts(&c.body, f);
                }
                if let Some(fin) = finally {
                    walk_stmts(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_stmts(&c.body, f);
                }
            }
            Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body.as_ref()), f),
            _ => {}
        }
    }
}

/// The `const r = A[i]` bindings this region may seed as rule-1 provenance.
///
/// The site test is [`collect_element_shape_facts`] above; this only pairs the
/// ids it licensed with the `Stmt::Let` that binds them, so `ptr_shape.rs`
/// never seeds a candidate for an id with no binding in the region it is
/// analysing.
pub(super) fn element_read_seeds(
    stmts: &[Stmt],
    element_facts: &ElementShapeFacts,
) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if element_facts.is_empty() {
        return out;
    }
    // Reuses this module's one statement walker rather than carrying a second
    // copy of the traversal. Two traversals that have to agree, drifting by
    // one `Stmt` variant, is a bug class this file cannot afford: a missed
    // variant here silently withholds a SEED, and a missed variant in
    // `ArrayWalk` silently withholds a DISQUALIFICATION.
    walk_stmts(stmts, &mut |s| {
        if let Stmt::Let {
            id,
            init: Some(Expr::IndexGet { .. }),
            ..
        } = s
        {
            if let Some(class_name) = element_facts.element_read_class(*id) {
                out.push((*id, class_name.to_string()));
            }
        }
    });
    out
}
#[cfg(test)]
#[path = "ptr_shape_elements_tests.rs"]
mod tests;
