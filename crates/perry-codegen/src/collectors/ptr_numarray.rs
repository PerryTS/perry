//! Representation-selection Phase 4a.3 (RFC `docs/representation-selection-rfc.md`
//! §4 `Array<number>` row, §5.7): numeric-array locals proven for GUARD-FREE
//! element access (`Ptr<NumArray>`).
//!
//! ## What this proves
//!
//! A function-local `let a = new Array(n)` / `let a: number[] = []` qualifies
//! as a **numeric-array pointer local** when static analysis proves that for
//! the local's entire lifetime:
//!
//! 1. every element slot in `[0, length)` holds canonical raw-f64 number bits
//!    or `TAG_HOLE` (never a NaN-boxed pointer/string/bool/undefined), and
//! 2. `length` never shrinks below the allocation length, and
//! 3. the binding can never go stale (no growth path exists that fails to
//!    write the live head back to the local slot).
//!
//! Element accesses at sites with an additional per-site in-bounds proof then
//! lower to the bare form — slot reload → mask → `gep +8` → `gep idx<<3` →
//! `load`/`store double` — with NO guard (not even the Phase 4a.1 inline
//! header tests), no barrier, no layout note, and no bounds check.
//!
//! ## Why it is sound (provenance + containment + density)
//!
//! * **Provenance**: the local is initialized by exactly one `Stmt::Let` whose
//!   init is `new Array(<static n>)` (runtime hole-fills every slot, sets
//!   `GC_ARRAY_RAW_F64_HOLES`, and stamps the pointer-free GC layout —
//!   `js_array_constructor_single`) or an EMPTY array literal `[]` (length 0;
//!   nothing to observe until a numeric push). Density: `new Array(n)` ⇒
//!   `HolesOK`; `[]` ⇒ `Dense`.
//! * **Containment**: every use of the local is an element read, an element
//!   write whose VALUE is numeric-by-construction (a non-number store would
//!   break invariant 1 — such a local is disqualified outright, not just
//!   per-site), a numeric-key `PutValueSet` on itself, `.length`, a numeric
//!   `push`, or a bare `return <local>` (the alias escapes only when the
//!   function is DONE — no in-function access can race it). Anything else —
//!   reassignment, any other bare reference (call arguments, `console.log`,
//!   object/array element stores, `JSON.stringify`), closure capture,
//!   `pop`/`shift`/`splice`/`unshift`/`copyWithin` (length shrink /
//!   reordering), freeze/seal — disqualifies. Non-numeric STRING keys could
//!   name `length` (shrink) or `__proto__`, so keyed writes require a
//!   provably-numeric key (a number key can only ever name an element or a
//!   harmless numeric-string property).
//! * **Self-heal exemption (the #6915 stale-binding finding)**: guard-free
//!   consumers have no runtime check that could catch a growth-forwarded
//!   stub, so the proof must exclude staleness STRUCTURALLY: containment
//!   means no callee ever receives the array (the specialized-ABI
//!   caller-allocated growth pattern cannot occur), every in-function growth
//!   site (push tiers, sparse-extend runtime arm) writes the live head back
//!   to the local slot, and every consumer re-derives the base from the slot
//!   (the slot address is shadow-bound, so LLVM cannot cache the load across
//!   calls; GC evacuation rewrites the slot through the same binding).
//! * **In-bounds per site** (checked at lowering time, not here): the index
//!   has `int_range` proof `[0, max]` with `max < proven_initial_length`
//!   (length can only grow — `pop`-class shrinkers are disqualified — so
//!   `idx < initial_length <= current length` holds forever), or the site is
//!   a `bounded_index_pairs` loop read. Everything unproven falls back to the
//!   Phase 4a.1/4a.2 guarded tiers, which maintain the same invariants.
//! * **Hole observability (density gating)**: guard-free READS are emitted
//!   only in ToNumber contexts (the Phase 4a number-context reader), where a
//!   `TAG_HOLE` load canonicalized to the quiet NaN is bit-exact with
//!   `ToNumber(undefined)`. Bare hole-OBSERVING reads (`x = a[i]` printed,
//!   `in`/`Object.keys`/`JSON.stringify` — the latter are call-argument
//!   escapes and disqualify anyway) stay on the guarded tiers.
//! * **Module-wide barrier kill**: reuses Phase 3b's §5.2 barrier scan
//!   (`Object.defineProperty` family, ANY `delete`, `setPrototypeOf` /
//!   `__proto__` writes, `Proxy`, mutating `Reflect.*`) PLUS the
//!   array-specific rule: any indexed write through a `.prototype` object
//!   anywhere in the module (`Array.prototype[0] = …`) disables all
//!   promotion — a polluted prototype changes what a HOLE read observes, and
//!   the guard-free read cannot consult the runtime pollution byte. Same
//!   module-granularity increment boundary as Phase 3b.
//!
//! Gated by `PERRY_PTR_NUMARRAY_LOCALS` (default on; `0`/`off`/`false`
//! disables — keyed into the object cache). `PERRY_REPSEL_DEBUG=1` prints one
//! line per proven local at compile time.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, LogicalOp, Stmt, UnaryOp};

use super::ModuleDispatchFacts;

/// `PERRY_PTR_NUMARRAY_LOCALS` gate. Enabled by default; `=0`/`off`/`false`
/// disables numeric-array pointer-local selection (every access keeps the
/// Phase 4a.1/4a.2 guarded tiers). Keyed into the object cache
/// (`object_cache.rs`).
pub fn ptr_numarray_locals_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_PTR_NUMARRAY_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn repsel_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("PERRY_REPSEL_DEBUG").as_deref() == Ok("1"))
}

/// Density lattice `Dense ⊒ HolesOK` (⊒ `Boxed` = not collected at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumArrayDensity {
    /// No hole can ever exist in `[0, length)` (empty-literal provenance;
    /// growth only through numeric pushes). Guard-free reads may skip the
    /// NaN canonicalization entirely.
    Dense,
    /// Slots are raw-f64-or-`TAG_HOLE` (`new Array(n)` provenance). Guard-free
    /// reads are number-context only and canonicalize `TAG_HOLE`/NaN payloads
    /// to the quiet NaN (≡ `ToNumber(undefined)` / `ToNumber(NaN)`).
    HolesOk,
}

/// A function-local proven to satisfy the `Ptr<NumArray>` invariants for its
/// entire lifetime. See the module doc for the proof obligations.
#[derive(Debug, Clone)]
pub struct NumArrayLocal {
    pub density: NumArrayDensity,
    /// The static allocation length. Because every length-shrinking operation
    /// disqualifies, `current_length >= proven_initial_length` holds forever,
    /// so `index < proven_initial_length` is a permanent in-bounds proof.
    pub proven_initial_length: i64,
}

/// Module-wide array barrier (in ADDITION to
/// [`super::ptr_shape::expr_is_shape_barrier`]): an indexed write through any
/// `.prototype` object. `Array.prototype[3] = x` makes a HOLE read observable
/// through the chain; the guard-free read cannot consult the runtime
/// pollution byte, so any such site in the module kills all promotion.
pub(crate) fn expr_is_numarray_prototype_index_barrier(expr: &Expr) -> bool {
    fn is_prototype_object(e: &Expr) -> bool {
        matches!(e, Expr::PropertyGet { property, .. } if property == "prototype")
    }
    match expr {
        Expr::IndexSet { object, .. } => is_prototype_object(object),
        Expr::PutValueSet { target, .. } => is_prototype_object(target),
        _ => false,
    }
}

/// Compile-time visibility: one stderr line per proven local, plus a
/// process-wide running count. Only under `PERRY_REPSEL_DEBUG=1`.
fn note_num_array_local(id: u32, fact: &NumArrayLocal) {
    if !repsel_debug_enabled() {
        return;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!(
        "repsel: ptr-numarray local id {id} density {:?} initial_len {} (total {n})",
        fact.density, fact.proven_initial_length
    );
}

/// Entry point: collect the `Ptr<NumArray>` locals of one lowered region.
pub(crate) fn collect_num_array_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    module_dispatch: &ModuleDispatchFacts,
    compile_time_constants: &HashMap<u32, f64>,
) -> HashMap<u32, NumArrayLocal> {
    if !ptr_numarray_locals_enabled()
        || module_dispatch.has_shape_barrier_sites()
        || module_dispatch.has_numarray_prototype_index_barriers()
    {
        return HashMap::new();
    }
    // Pass 1: single-`Let` provenance candidates.
    let mut walk = UseWalk {
        boxed_vars,
        module_globals,
        compile_time_constants,
        candidates: HashMap::new(),
        let_counts: HashMap::new(),
        let_types: HashMap::new(),
        disqualified: HashSet::new(),
    };
    walk.collect_candidates(stmts);
    if walk.candidates.is_empty() {
        return HashMap::new();
    }
    // Pass 2: strict use walk (containment + numeric-write proof).
    walk.walk_stmts(stmts);
    let UseWalk {
        candidates,
        let_counts,
        disqualified,
        ..
    } = walk;
    let mut out = HashMap::new();
    for (id, fact) in candidates {
        if disqualified.contains(&id) || let_counts.get(&id).copied().unwrap_or(0) != 1 {
            continue;
        }
        note_num_array_local(id, &fact);
        out.insert(id, fact);
    }
    out
}

struct UseWalk<'a> {
    boxed_vars: &'a HashSet<u32>,
    module_globals: &'a HashMap<u32, String>,
    compile_time_constants: &'a HashMap<u32, f64>,
    candidates: HashMap<u32, NumArrayLocal>,
    let_counts: HashMap<u32, u32>,
    /// Declared `Let` types in this region (numeric-key / numeric-value
    /// trust, matching the project-wide annotation-trust precedent).
    let_types: HashMap<u32, HirType>,
    disqualified: HashSet<u32>,
}

impl<'a> UseWalk<'a> {
    /// Resolve a static non-negative array-allocation length: an integer
    /// literal or a module-level `const` recorded in `compile_time_constants`.
    fn static_alloc_length(&self, e: &Expr) -> Option<i64> {
        let value = match e {
            Expr::Integer(v) => *v as f64,
            Expr::Number(v) => *v,
            Expr::LocalGet(id) => *self.compile_time_constants.get(id)?,
            _ => return None,
        };
        if !value.is_finite() || value.fract() != 0.0 || !(0.0..=16_000_000.0).contains(&value) {
            return None;
        }
        Some(value as i64)
    }

    fn provenance(&self, init: &Expr) -> Option<NumArrayLocal> {
        match init {
            // `new Array(<static n>)`: runtime hole-fills, sets the
            // raw-f64-or-holes flag, stamps pointer-free layout.
            Expr::New {
                class_name, args, ..
            } if class_name == "Array" => match args.as_slice() {
                [] => Some(NumArrayLocal {
                    density: NumArrayDensity::Dense,
                    proven_initial_length: 0,
                }),
                [len] => Some(NumArrayLocal {
                    density: NumArrayDensity::HolesOk,
                    proven_initial_length: self.static_alloc_length(len)?,
                }),
                _ => None,
            },
            // Empty array literal: length 0, nothing to observe until a
            // (numeric-only, by containment) push.
            Expr::Array(elems) if elems.is_empty() => Some(NumArrayLocal {
                density: NumArrayDensity::Dense,
                proven_initial_length: 0,
            }),
            _ => None,
        }
    }

    fn collect_candidates(&mut self, stmts: &[Stmt]) {
        for_each_stmt(stmts, &mut |s| {
            if let Stmt::Let { id, ty, init, .. } = s {
                self.let_types.insert(*id, ty.clone());
                if self.boxed_vars.contains(id) || self.module_globals.contains_key(id) {
                    return;
                }
                // Only statically array-of-number bindings: the declared type
                // is the same trust boundary every numeric fast path uses.
                let numeric_array_ty = match ty {
                    HirType::Array(elem) => {
                        matches!(elem.as_ref(), HirType::Number | HirType::Int32)
                    }
                    HirType::Generic { base, type_args } if base == "Array" => {
                        type_args.len() == 1
                            && matches!(type_args[0], HirType::Number | HirType::Int32)
                    }
                    _ => false,
                };
                if !numeric_array_ty {
                    return;
                }
                if let Some(init) = init {
                    if let Some(fact) = self.provenance(init) {
                        self.candidates.insert(*id, fact);
                    }
                }
            }
        });
    }

    fn disq(&mut self, id: u32) {
        self.disqualified.insert(id);
    }

    fn is_candidate(&self, id: u32) -> bool {
        self.candidates.contains_key(&id)
    }

    /// A key that can only ever name an element or a harmless numeric-string
    /// property — never `"length"` / `"__proto__"` (ToString of a number can
    /// name neither).
    fn key_is_provably_numeric(&self, key: &Expr) -> bool {
        match key {
            Expr::Integer(_) | Expr::Number(_) => true,
            Expr::LocalGet(id) => matches!(
                self.let_types.get(id),
                Some(HirType::Number) | Some(HirType::Int32)
            ),
            _ => self.value_is_numeric(u32::MAX, key),
        }
    }

    /// Numeric-by-construction for stored values: the runtime value is a JS
    /// number for every input. `tracked` element reads are number|undefined;
    /// they are numeric as arithmetic operands (undefined → NaN) and as the
    /// LEFT side of `||`/`??` (undefined never passes through), but NOT bare
    /// and NOT under `&&` (a falsy `undefined` left IS the result).
    fn value_is_numeric(&self, tracked: u32, e: &Expr) -> bool {
        match e {
            Expr::Integer(_) | Expr::Number(_) => true,
            Expr::NumberCoerce(_) => true,
            Expr::LocalGet(id) => {
                self.compile_time_constants.contains_key(id)
                    || matches!(
                        self.let_types.get(id),
                        Some(HirType::Number) | Some(HirType::Int32)
                    )
            }
            Expr::Unary { op, operand } => {
                matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot)
                    && self.value_or_element_is_numeric(tracked, operand)
            }
            Expr::Binary { op, left, right } => {
                matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Shr
                        | BinaryOp::UShr
                ) && self.value_or_element_is_numeric(tracked, left)
                    && self.value_or_element_is_numeric(tracked, right)
            }
            Expr::Logical { op, left, right } => {
                matches!(op, LogicalOp::Or | LogicalOp::Coalesce)
                    && self.value_or_element_is_numeric(tracked, left)
                    && self.value_is_numeric(tracked, right)
            }
            Expr::MathFloor(..)
            | Expr::MathCeil(..)
            | Expr::MathRound(..)
            | Expr::MathTrunc(..)
            | Expr::MathAbs(..)
            | Expr::MathSqrt(..)
            | Expr::MathMin(..)
            | Expr::MathMax(..)
            | Expr::MathPow(..)
            | Expr::MathImul(..)
            | Expr::MathRandom => true,
            _ => false,
        }
    }

    /// [`Self::value_is_numeric`] extended with "an element read of the
    /// tracked array itself" (number|undefined — valid exactly where a
    /// ToNumber/short-circuit context absorbs the undefined).
    fn value_or_element_is_numeric(&self, tracked: u32, e: &Expr) -> bool {
        if let Expr::IndexGet { object, index } = e {
            if let Expr::LocalGet(id) = object.as_ref() {
                if *id == tracked && self.key_is_provably_numeric(index) {
                    return true;
                }
            }
        }
        self.value_is_numeric(tracked, e)
    }

    fn walk_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                if self.is_candidate(*id) {
                    *self.let_counts.entry(*id).or_insert(0) += 1;
                    // The provenance init itself contains no use of the local.
                    return;
                }
                if let Some(e) = init {
                    self.walk_expr(e);
                }
            }
            // A bare `return <local>` hands the alias out only when the
            // function is DONE on that path — no later in-function access can
            // observe an external mutation through it.
            Stmt::Return(Some(Expr::LocalGet(id))) if self.is_candidate(*id) => {}
            Stmt::Expr(e) | Stmt::Throw(e) => self.walk_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.walk_expr(e);
                }
            }
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
                if let Some(init) = init {
                    self.walk_stmt(init.as_ref());
                }
                if let Some(c) = condition {
                    self.walk_expr(c);
                }
                if let Some(u) = update {
                    self.walk_expr(u);
                }
                self.walk_stmts(body);
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

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            // Element read: safe when the key is provably numeric (a
            // non-numeric key is an ordinary [[Get]] — harmless for the
            // element invariants, but the KEY expression may itself contain a
            // bare reference; keep it simple and require numeric).
            Expr::IndexGet { object, index } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.is_candidate(*id) {
                        if !self.key_is_provably_numeric(index) {
                            self.disq(*id);
                        }
                        self.walk_expr(index);
                        return;
                    }
                }
                self.walk_expr(object);
                self.walk_expr(index);
            }
            // Element write: numeric key + numeric-by-construction value, or
            // the local is disqualified outright (a single non-number store
            // would break the slot invariant for every guard-free read).
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.is_candidate(*id) {
                        if !self.key_is_provably_numeric(index)
                            || !self.value_is_numeric(*id, value)
                        {
                            self.disq(*id);
                        }
                        self.walk_expr(index);
                        self.walk_expr(value);
                        return;
                    }
                }
                self.walk_expr(object);
                self.walk_expr(index);
                self.walk_expr(value);
            }
            // Sloppy-mode `a[k] = v` desugars to PutValueSet with
            // target == receiver == the local.
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                if let (Expr::LocalGet(id), Expr::LocalGet(rid)) =
                    (target.as_ref(), receiver.as_ref())
                {
                    if id == rid && self.is_candidate(*id) {
                        if !self.key_is_provably_numeric(key) || !self.value_is_numeric(*id, value)
                        {
                            self.disq(*id);
                        }
                        self.walk_expr(key);
                        self.walk_expr(value);
                        return;
                    }
                }
                self.walk_expr(target);
                self.walk_expr(key);
                self.walk_expr(value);
                self.walk_expr(receiver);
            }
            // `.length` read on the local is safe. Any other property
            // access (methods like `sort`/`slice`, `length` WRITES come
            // through PropertySet) disqualifies.
            Expr::PropertyGet {
                object, property, ..
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.is_candidate(*id) {
                        if property != "length" {
                            self.disq(*id);
                        }
                        return;
                    }
                }
                self.walk_expr(object);
            }
            Expr::PropertySet { object, value, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.is_candidate(*id) {
                        // `a.length = n` (shrink!) or any expando.
                        self.disq(*id);
                    }
                } else {
                    self.walk_expr(object);
                }
                self.walk_expr(value);
            }
            Expr::PropertyUpdate { object, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if self.is_candidate(*id) {
                        self.disq(*id);
                        return;
                    }
                }
                self.walk_expr(object);
            }
            // Numeric push keeps every invariant (canonical store through the
            // Phase 4a.1 tiers; growth writes the live head back to the
            // slot). A possibly-non-numeric push value disqualifies.
            Expr::ArrayPush { array_id, value } => {
                if self.is_candidate(*array_id) && !self.value_is_numeric(*array_id, value) {
                    self.disq(*array_id);
                }
                self.walk_expr(value);
            }
            // Length-shrinking / reordering / hole-materializing mutators.
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                if self.is_candidate(*id) {
                    self.disq(*id);
                }
            }
            Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                self.disq(*array_id);
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
            Expr::ObjectFreeze(t) | Expr::ObjectSeal(t) | Expr::ObjectPreventExtensions(t) => {
                if let Expr::LocalGet(id) = t.as_ref() {
                    if self.is_candidate(*id) {
                        self.disq(*id);
                        return;
                    }
                }
                self.walk_expr(t);
            }
            // Reassignment / bare reference / numeric update = escape or
            // rebinding — either way the proof is gone.
            Expr::LocalSet(id, v) => {
                if self.is_candidate(*id) {
                    self.disq(*id);
                }
                self.walk_expr(v);
            }
            Expr::LocalGet(id) => {
                if self.is_candidate(*id) {
                    self.disq(*id);
                }
            }
            Expr::Update { id, .. } => {
                if self.is_candidate(*id) {
                    self.disq(*id);
                }
            }
            // Closures: captured or body-referenced candidates escape.
            Expr::Closure {
                body,
                captures,
                mutable_captures,
                ..
            } => {
                for c in captures.iter().chain(mutable_captures.iter()) {
                    if self.is_candidate(*c) {
                        self.disq(*c);
                    }
                }
                self.walk_stmts(body);
            }
            // Everything else: recurse; a bare LocalGet of a candidate in any
            // unhandled position hits the LocalGet arm above and escapes.
            _ => {
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
        }
    }
}

/// Depth-first statement visitor (Let-collection pre-pass).
fn for_each_stmt(stmts: &[Stmt], f: &mut impl FnMut(&Stmt)) {
    fn go(stmts: &[Stmt], f: &mut impl FnMut(&Stmt)) {
        for s in stmts {
            f(s);
            match s {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    go(then_branch, f);
                    if let Some(eb) = else_branch {
                        go(eb, f);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => go(body, f),
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        go(std::slice::from_ref(init.as_ref()), f);
                    }
                    go(body, f);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    go(body, f);
                    if let Some(c) = catch {
                        go(&c.body, f);
                    }
                    if let Some(fin) = finally {
                        go(fin, f);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        go(&case.body, f);
                    }
                }
                Stmt::Labeled { body, .. } => go(std::slice::from_ref(body.as_ref()), f),
                _ => {}
            }
        }
    }
    go(stmts, f);
}
