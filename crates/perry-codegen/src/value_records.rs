//! Value records in codegen (DESIGN-p4l4 VR-3a, `PERRY_FACTS_VALUE_RECORDS=1`).
//!
//! A local the facts give `Rep::Record` (`ElisionFacts::record_locals`) is
//! its fields by value: before lowering, each such binding becomes one local
//! per layout key (plus a presence local when it may be `undefined`), and
//! every use of the binding becomes one of:
//!
//! - a field read `r.k`: the field's local (§5.1);
//! - a presence test (`=== undefined`, `== null`, truthiness, `typeof`,
//!   `&&`, `||`, `??`): the presence local (§5.1);
//! - an assignment: the field locals are written from the record value
//!   (a literal birth, another record local, `undefined`, a conditional of
//!   those), and the one form the facts leave, a value the program holds
//!   boxed, is read key by key;
//! - every other use, which the facts admit only at a sanctioned boundary
//!   (§4.1-4.3: a native that reads or coerces the argument by a fixed
//!   protocol, a boundary-only wrapper) or at a helper's Boxed parameter
//!   (owner ruling 2026-11-10, §4.4): the materializer (§4.2), a fresh
//!   object born by the template's own literal birth from the field locals.
//!
//! So no lowering ever sees a record binding: there is no record value that
//! a runtime call could receive (§6.3 part 1), and the guarded generic arm
//! of a proof (`wpfacts_off`) lowers the same rewritten body (§5.5: records
//! are representations, not proofs). Check mode claims each materialized
//! copy at its birth (VR-2's `record_witness::claim`), so a compiled
//! identity comparison of a copy is a false proof (rc 86).
//!
//! The rewrite is total or nothing per binding: a binding declared once by
//! a `let` in a statement list, referenced only by `LocalGet`/`LocalSet`
//! in the bodies the rewrite visits (no closure capture, no other id-bearing
//! node, not a module global, not exported), each reference after its
//! declaration. A binding that is not is left as it is, a heap object; the
//! values that flow between such a binding and a rewritten one take the
//! materializer or the key-by-key read, both sound for an immutable,
//! identity-free template (R2, R4).
//!
//! VR-3b, a record across a typed call (§5.2): the signature slot the facts
//! give `Record` (`FactsSignature::record_params`, `record_ret`) carries the
//! record as its lanes, one per layout word plus a presence lane. The rewrite
//! writes each side in a form that is also the boxed program, so a callee
//! codegen gives no `$sig` still runs:
//!
//! - a record parameter is a binding declared at the body's entry, its
//!   fields read from the parameter key by key (the prologue, which
//!   `record_sig::sig_body` replaces by the lane parameters of `F$sig`);
//! - `return e` of a record-returning body writes `e` into the body's result
//!   lanes and returns their copy (`record_sig::lower_return` returns the
//!   lanes);
//! - an argument in a record slot is the copy of a binding's lanes
//!   (`record_sig::copy_lanes` reads them back: no object is born);
//! - a call that returns a record is bound to an aggregate temporary
//!   (`record_aggs`): the typed call writes the result lanes into the
//!   destination's field locals and leaves `true` (or `undefined`) in the
//!   temporary; a boxed result is read key by key.
//!
//! VR-4b, record arrays (§1.4, §5.3): an Array the facts name a record array
//! (`ElisionFacts::record_array_*`) is a `GC_TYPE_RECORD_ARRAY`, its
//! elements the records' words inline. Only these forms touch one, each a
//! `__perry_runtime` intrinsic `codegen::record_array` lowers:
//!
//! - its birth, an empty literal that is the whole value of its holder
//!   (`let x = []`, `x = []`, `o.k = []`, a class field initializer);
//! - `push(r)`: the copy of `r`'s lanes is stored (no object is born);
//! - `pop()` and `a[i]`: record results, bound to lanes as a typed call's
//!   (the aggregate temporary; absent when empty or out of bounds);
//! - `length`;
//! - `for (const r of a)`: the index arm of HIR's for-of
//!   (`perry_hir::lower::for_of_guard`), whose iterator arm is dropped.
//!
//! A receiver is typed by its binding or by its named read; a binding
//! initialized or assigned from one is typed with it. Any other use of a
//! typed receiver that is no plain value flow is a compile error: the facts
//! reject every operation these forms do not cover (`R6Op`), so reaching one
//! means the two sides disagree, and an ordinary Array lowering of a record
//! array would read another object's layout.
use std::collections::{HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::{CompareOp, Expr, Function, LogicalOp, Module, Stmt, UnaryOp};

use crate::codegen::{ElisionFacts, FactsRecRep, FactsRecordFn, FactsRecordLanes, FactsSignature};

/// How a template's literal is born, and so how a copy is materialized.
#[derive(Clone, Debug)]
enum Birth {
    /// `new __AnonShape_*(…)`: the class, its fields in constructor order.
    Class { name: String, fields: Vec<String> },
    /// An `Expr::Object` literal with the layout keys in order.
    Object,
}

#[derive(Clone, Debug)]
struct Shape {
    birth: Birth,
    /// The layout keys, in birth order.
    keys: Vec<String>,
    /// Per key: a Number lane (its filler is `0`, else `undefined`).
    numeric: Vec<bool>,
}

/// A rewritten binding: its field locals (layout order) and the presence
/// local, which is tracked when the binding may hold `undefined`.
#[derive(Clone, Debug)]
struct Rec {
    tmpl: u32,
    track: bool,
    present: LocalId,
    fields: Vec<LocalId>,
    /// A boxed value read key by key into the fields.
    tmp: LocalId,
    /// Each field's next value, when a literal assigned to the binding
    /// reads the binding (every value is evaluated before a field is
    /// written).
    next: Vec<LocalId>,
    /// The aggregate temporary a record-returning typed call is bound to
    /// (VR-3b).
    agg: LocalId,
}

/// What the rewrite tells the lowering of the record signature ABI
/// (`ElisionFacts::record_fns`, `record_aggs`, `record_births`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Abi {
    pub fns: std::collections::BTreeMap<u32, FactsRecordFn>,
    pub aggs: std::collections::BTreeMap<u32, FactsRecordLanes>,
    pub births: std::collections::BTreeMap<u32, crate::codegen::FactsRecordBirth>,
}

/// Per-module counts, for `PERRY_VALUE_RECORDS_TRACE=1` and the probes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Census {
    /// Bindings rewritten to their fields.
    pub locals: usize,
    /// Bindings the facts give a record that stay heap objects (the rule of
    /// the module docs).
    pub kept: usize,
    pub field_reads: usize,
    pub presence: usize,
    /// Materialized copies (§4.2), each at a boundary or a Boxed slot.
    pub materialized: usize,
    /// Boxed values read key by key into a binding's fields.
    pub unpacked: usize,
    /// VR-3b: record parameters bound at a body's entry, bodies that return
    /// a record through their result lanes, arguments passed in a record
    /// slot, and record-returning typed calls bound to lanes.
    pub params: usize,
    pub returns: usize,
    pub passed: usize,
    pub calls: usize,
    /// VR-4b: record-array births and operations lowered as intrinsics.
    pub arrays: usize,
    pub array_ops: usize,
}

/// The module with every eligible record binding rewritten, or `None`
/// when there is none (the module lowers as it is).
pub(crate) fn rewrite(hir: &Module, facts: &ElisionFacts) -> Option<(Module, Census, Abi)> {
    let arrays = facts.has_record_arrays();
    if facts.record_locals.is_empty() && !facts.signatures.values().any(has_record) && !arrays {
        return None;
    }
    let shapes = shapes(hir, facts);
    let ra_locals = record_array_locals(hir, facts, &shapes);
    let cands: HashMap<LocalId, FactsRecRep> = facts
        .record_locals
        .iter()
        .filter(|(_, r)| shapes.contains_key(&r.tmpl))
        .map(|(id, r)| (*id, *r))
        .collect();
    if cands.is_empty() && !facts.signatures.values().any(has_record) && !arrays {
        return None;
    }
    // VR-3b: the functions whose record slots cross as lanes, and their
    // parameters in a `Record` signature slot, which are bindings the
    // body's entry declares.
    let lane_fns = lane_functions(hir, facts);
    let record_sigs = !lane_fns.is_empty();
    let mut params: HashSet<LocalId> = HashSet::new();
    for f in all_functions(hir) {
        let Some(sig) = facts
            .signatures
            .get(&f.id)
            .filter(|_| lane_fns.contains(&f.id))
        else {
            continue;
        };
        for (p, r) in f.params.iter().zip(&sig.record_params) {
            if r.is_some_and(|r| cands.get(&p.id).is_some_and(|c| c.tmpl == r.tmpl))
                && p.default.is_none()
                && !p.is_rest
                && p.arguments_object.is_none()
            {
                params.insert(p.id);
            }
        }
    }
    let census = Refs::of(hir, &cands, &params);
    let mut eligible: HashSet<LocalId> = cands
        .keys()
        .copied()
        .filter(|id| {
            !census.bad.contains(id)
                && if params.contains(id) {
                    !census.lets.contains_key(id)
                } else {
                    census.lets.get(id) == Some(&1)
                }
        })
        .collect();
    let next = perry_transform::generator::compute_max_local_id(hir).saturating_add(1);
    // A binding whose references the rewrite did not all reach stays a
    // heap object; the others are rewritten again without it.
    while !eligible.is_empty() || record_sigs || arrays {
        let mut out = hir.clone();
        let mut rw = Rewriter {
            shapes: &shapes,
            cands: &cands,
            eligible: &eligible,
            facts,
            lane_fns: &lane_fns,
            ra_locals: &ra_locals,
            recs: HashMap::new(),
            next,
            reached: HashMap::new(),
            failed: HashSet::new(),
            census: Census::default(),
            ret: None,
            pending: Vec::new(),
            abi: Abi::default(),
        };
        rw.module(&mut out);
        let dropped: HashSet<LocalId> = eligible
            .iter()
            .copied()
            .filter(|id| {
                rw.failed.contains(id)
                    || !rw.recs.contains_key(id)
                    || rw.reached.get(id).copied().unwrap_or(0)
                        != census.refs.get(id).copied().unwrap_or(0)
            })
            .collect();
        if dropped.is_empty() {
            let mut c = rw.census;
            c.locals = eligible.len();
            c.kept = cands.len() - eligible.len();
            if c == (Census {
                kept: c.kept,
                ..Census::default()
            }) {
                break;
            }
            trace(hir, &c);
            let mut abi = rw.abi;
            for (&tmpl, shape) in &shapes {
                abi.births.insert(tmpl, shape.birth_fact());
            }
            return Some((out, c, abi));
        }
        eligible.retain(|id| !dropped.contains(id));
    }
    trace(
        hir,
        &Census {
            kept: cands.len(),
            ..Census::default()
        },
    );
    None
}

/// `rewrite`, with what it tells the record signature lowering
/// (`codegen::record_sig`) installed in the module's facts.
pub(crate) fn rewrite_module(
    hir: &Module,
    facts: &mut std::sync::Arc<ElisionFacts>,
) -> Option<Module> {
    let (module, _, abi) = rewrite(hir, facts)?;
    let mut elision = (**facts).clone();
    elision.record_fns = abi.fns;
    elision.record_aggs = abi.aggs;
    elision.record_births = abi.births;
    *facts = std::sync::Arc::new(elision);
    Some(module)
}

fn trace(hir: &Module, c: &Census) {
    if std::env::var("PERRY_VALUE_RECORDS_TRACE").as_deref() == Ok("1") {
        eprintln!(
            "perry: value records: module {}: locals {} kept {} field reads {} presence {} materialized {} unpacked {} params {} returns {} passed {} calls {}",
            hir.name,
            c.locals,
            c.kept,
            c.field_reads,
            c.presence,
            c.materialized,
            c.unpacked,
            c.params,
            c.returns,
            c.passed,
            c.calls
        );
    }
}

/// Each admitted template with a record binding, and how its literal is
/// born in this module.
fn shapes(hir: &Module, facts: &ElisionFacts) -> HashMap<u32, Shape> {
    let mut out = HashMap::new();
    for (&tmpl, layout) in &facts.record_layouts {
        let birth = match layout.class_id {
            Some(cid) => {
                let Some(class) = hir
                    .classes
                    .iter()
                    .find(|c| c.id == cid && c.is_literal_shape())
                else {
                    continue;
                };
                let fields: Vec<String> = class.fields.iter().map(|f| f.name.clone()).collect();
                let mut a = fields.clone();
                let mut b = layout.keys.clone();
                a.sort();
                b.sort();
                if a != b {
                    continue;
                }
                Birth::Class {
                    name: class.name.clone(),
                    fields,
                }
            }
            None => Birth::Object,
        };
        out.insert(
            tmpl,
            Shape {
                birth,
                keys: layout.keys.clone(),
                numeric: layout.numeric.clone(),
            },
        );
    }
    out
}

/// The module-wide references of the candidates, every expression and
/// statement visited (`perry_hir::tdz_check` walkers).
struct Refs {
    /// `LocalGet` / `LocalSet` occurrences.
    refs: HashMap<LocalId, usize>,
    /// `let` declarations in statement lists.
    lets: HashMap<LocalId, usize>,
    /// Named by an id-bearing node other than `LocalGet`/`LocalSet`, a
    /// closure capture, a module global, an export, a catch parameter or a
    /// box list.
    bad: HashSet<LocalId>,
}

impl Refs {
    fn of(hir: &Module, cands: &HashMap<LocalId, FactsRecRep>, params: &HashSet<LocalId>) -> Refs {
        let mut r = Refs {
            refs: HashMap::new(),
            lets: HashMap::new(),
            bad: HashSet::new(),
        };
        let on_expr = |e: &Expr, r: &mut Refs| {
            let bad = |id: &LocalId, r: &mut Refs| {
                if cands.contains_key(id) {
                    r.bad.insert(*id);
                }
            };
            match e {
                Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
                    if cands.contains_key(id) {
                        *r.refs.entry(*id).or_default() += 1;
                    }
                }
                Expr::Update { id, .. }
                | Expr::ScopedTemp { id, .. }
                | Expr::ArrayPop(id)
                | Expr::ArrayShift(id)
                | Expr::ArrayPush { array_id: id, .. }
                | Expr::ArrayPushSpread { array_id: id, .. }
                | Expr::ArrayUnshift { array_id: id, .. }
                | Expr::ArraySplice { array_id: id, .. }
                | Expr::ArrayCopyWithin { array_id: id, .. }
                | Expr::SetAdd { set_id: id, .. } => bad(id, r),
                Expr::ClassExprFresh {
                    evaluation_owner: Some(id),
                    ..
                } => bad(id, r),
                Expr::LinkGeneratorPrototype {
                    i32_controls,
                    i1_controls,
                    ..
                } => {
                    for id in i32_controls.iter().chain(i1_controls) {
                        bad(id, r);
                    }
                }
                Expr::Closure {
                    captures,
                    mutable_captures,
                    params,
                    ..
                } => {
                    for id in captures.iter().chain(mutable_captures) {
                        bad(id, r);
                    }
                    for p in params {
                        bad(&p.id, r);
                    }
                }
                _ => {}
            }
        };
        perry_hir::tdz_check::for_each_module_expr(hir, &mut |e| on_expr(e, &mut r));
        for g in &hir.globals {
            if cands.contains_key(&g.id) {
                r.bad.insert(g.id);
            }
            if let Some(init) = &g.init {
                perry_hir::tdz_check::for_each_expr_in(init, &mut |e| on_expr(e, &mut r));
            }
        }
        perry_hir::tdz_check::for_each_module_stmt_list(hir, &mut |list| {
            for s in list {
                match s {
                    Stmt::Let { id, .. } if cands.contains_key(id) => {
                        *r.lets.entry(*id).or_default() += 1;
                    }
                    Stmt::Try {
                        catch:
                            Some(perry_hir::CatchClause {
                                param: Some((id, _)),
                                ..
                            }),
                        ..
                    } if cands.contains_key(id) => {
                        r.bad.insert(*id);
                    }
                    Stmt::PreallocateBoxes(ids)
                    | Stmt::PreallocateTdzBoxes(ids)
                    | Stmt::ReleaseBoxes(ids) => {
                        r.bad
                            .extend(ids.iter().filter(|id| cands.contains_key(*id)));
                    }
                    _ => {}
                }
            }
        });
        for f in all_functions(hir) {
            for p in &f.params {
                if cands.contains_key(&p.id) && !params.contains(&p.id) {
                    r.bad.insert(p.id);
                }
            }
            for id in &f.captures {
                if cands.contains_key(id) {
                    r.bad.insert(*id);
                }
            }
        }
        // An exported binding has consumers no `LocalGet` here shows.
        let exported: HashSet<&str> = hir
            .exported_objects
            .iter()
            .map(String::as_str)
            .chain(hir.exports.iter().filter_map(|e| match e {
                perry_hir::Export::Named { local, .. } => Some(local.as_str()),
                _ => None,
            }))
            .collect();
        for s in &hir.init {
            if let Stmt::Let { id, name, .. } = s {
                if cands.contains_key(id) && exported.contains(name.as_str()) {
                    r.bad.insert(*id);
                }
            }
        }
        r
    }
}

/// Every function body of `hir`: functions and class members.
fn all_functions(hir: &Module) -> Vec<&Function> {
    let mut v: Vec<&Function> = hir.functions.iter().collect();
    for c in &hir.classes {
        v.extend(c.constructor.iter());
        v.extend(c.methods.iter().chain(&c.static_methods));
        v.extend(c.getters.iter().chain(&c.setters).map(|(_, f)| f));
        v.extend(c.computed_members.iter().map(|m| &m.function));
    }
    v
}

struct Rewriter<'a> {
    shapes: &'a HashMap<u32, Shape>,
    cands: &'a HashMap<LocalId, FactsRecRep>,
    eligible: &'a HashSet<LocalId>,
    facts: &'a ElisionFacts,
    /// The functions whose record slots cross as lanes (`lane_functions`).
    lane_fns: &'a HashSet<u32>,
    /// VR-4b: the bindings that hold a record array, and its record's
    /// template (`record_array_locals`).
    ra_locals: &'a HashMap<LocalId, u32>,
    /// The result lanes of the record-returning body being rewritten.
    ret: Option<LocalId>,
    /// Declarations of temporaries, placed before the statement in progress.
    pending: Vec<Stmt>,
    abi: Abi,
    /// The bindings declared so far, in traversal order.
    recs: HashMap<LocalId, Rec>,
    next: LocalId,
    /// References rewritten, per binding.
    reached: HashMap<LocalId, usize>,
    /// Bindings a use could not be rewritten for.
    failed: HashSet<LocalId>,
    census: Census,
}

/// VR-4b: the record-array intrinsics, `__perry_runtime` natives that
/// `codegen::record_array` lowers. Each names its record template as an
/// integer literal: `new(tmpl)`, `push(a, tmpl, copy)`, `pop(a, tmpl)`,
/// `get(a, tmpl, i)`, `length(a)`.
const RT: &str = "__perry_runtime";
pub(crate) const RA_NEW: &str = "valueRecordArrayNew";
pub(crate) const RA_PUSH: &str = "valueRecordArrayPush";
pub(crate) const RA_POP: &str = "valueRecordArrayPop";
pub(crate) const RA_GET: &str = "valueRecordArrayGet";
pub(crate) const RA_LENGTH: &str = "valueRecordArrayLength";

pub(crate) fn is_intrinsic(method: &str) -> bool {
    matches!(method, RA_NEW | RA_PUSH | RA_POP | RA_GET | RA_LENGTH)
}

fn rt(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: RT.into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}

fn is_empty_array(e: &Expr) -> bool {
    matches!(e, Expr::Array(v, _) if v.is_empty())
}

/// VR-4b: the bindings that hold a record array: the facts' own, and every
/// binding initialized or assigned from a typed receiver (a local a
/// transform bound a named read to carries no fact of its own).
fn record_array_locals(
    hir: &Module,
    facts: &ElisionFacts,
    shapes: &HashMap<u32, Shape>,
) -> HashMap<LocalId, u32> {
    let mut out: HashMap<LocalId, u32> = facts
        .record_array_locals
        .iter()
        .filter(|(_, t)| shapes.contains_key(t))
        .map(|(id, t)| (*id, *t))
        .collect();
    if !facts.has_record_arrays() {
        return out;
    }
    loop {
        let of = |e: &Expr, out: &HashMap<LocalId, u32>| match e {
            Expr::LocalGet(id) => out.get(id).copied(),
            Expr::PropertyGet {
                property,
                byte_offset,
                ..
            } if *byte_offset != 0 => facts
                .record_array_reads
                .get(&(*byte_offset, property.clone()))
                .copied()
                .filter(|t| shapes.contains_key(t)),
            _ => None,
        };
        let mut found: Vec<(LocalId, u32)> = Vec::new();
        perry_hir::tdz_check::for_each_module_stmt_list(hir, &mut |list| {
            for s in list {
                if let Stmt::Let {
                    id,
                    init: Some(e),
                    ..
                } = s
                {
                    if let Some(t) = of(e, &out) {
                        found.push((*id, t));
                    }
                }
            }
        });
        perry_hir::tdz_check::for_each_module_expr(hir, &mut |e| {
            if let Expr::LocalSet(id, v) = e {
                if let Some(t) = of(v, &out) {
                    found.push((*id, t));
                }
            }
        });
        let before = out.len();
        out.extend(found);
        if out.len() == before {
            return out;
        }
    }
}

fn undefined() -> Expr {
    Expr::Undefined
}

fn seq(mut list: Vec<Expr>, value: Expr) -> Expr {
    list.push(value);
    Expr::Sequence(list)
}

fn let_stmt(id: LocalId, name: String, ty: Type, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name,
        ty,
        mutable,
        init: Some(init),
    }
}

/// `undefined`, `void 0`.
fn is_undefined(e: &Expr) -> bool {
    match e {
        Expr::Undefined => true,
        Expr::Void(inner) => matches!(
            inner.as_ref(),
            Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Undefined | Expr::Null
        ),
        _ => false,
    }
}

impl Rewriter<'_> {
    fn fresh(&mut self) -> LocalId {
        let id = self.next;
        self.next += 1;
        id
    }

    fn rec_of(&self, e: &Expr) -> Option<LocalId> {
        match e {
            Expr::LocalGet(id) if self.recs.contains_key(id) => Some(*id),
            _ => None,
        }
    }

    fn reach(&mut self, id: LocalId) {
        *self.reached.entry(id).or_default() += 1;
    }

    fn module(&mut self, m: &mut Module) {
        self.stmts(&mut m.init);
        for f in &mut m.functions {
            self.function(f);
        }
        for c in &mut m.classes {
            // VR-4b: a field initializer that births a record array.
            for field in c.fields.iter_mut() {
                let key = (c.id, field.name.clone());
                if let (Some(tmpl), Some(e)) =
                    (self.facts.record_array_birth_fields.get(&key), &mut field.init)
                {
                    if is_empty_array(e) {
                        *e = self.ra_new(*tmpl);
                    }
                }
            }
            if let Some(f) = &mut c.constructor {
                self.function(f);
            }
            for f in c.methods.iter_mut().chain(c.static_methods.iter_mut()) {
                self.function(f);
            }
            for (_, f) in c.getters.iter_mut().chain(c.setters.iter_mut()) {
                self.function(f);
            }
            for member in &mut c.computed_members {
                self.function(&mut member.function);
            }
        }
    }

    fn function(&mut self, f: &mut Function) {
        for p in f.params.iter_mut() {
            if self.ra_locals.contains_key(&p.id) {
                p.ty = Type::Any;
            }
        }
        let facts: &ElisionFacts = self.facts;
        let sig = facts
            .signatures
            .get(&f.id)
            .filter(|s| has_record(s) && self.lane_fns.contains(&f.id));
        // The default prologues HIR lowering prepends stay first
        // (`signature_entry::sig_function` reads them there); the record
        // prologue follows, so no default may read a record parameter.
        let defaults: Vec<LocalId> = f
            .params
            .iter()
            .filter(|p| p.default.is_some())
            .map(|p| p.id)
            .collect();
        let at = defaults.len();
        let plain_prologue = defaults.iter().enumerate().all(|(i, id)| {
            f.body.get(i).is_some_and(|s| {
                crate::codegen::signature_entry::is_default_prologue(s, *id)
                    && !stmt_refers_to_any(s, &|x| self.eligible.contains(&x))
            })
        });
        let mut lanes: Vec<Option<FactsRecordLanes>> = vec![None; f.params.len()];
        // The lane declarations `F$sig` replaces by its parameters, then
        // the declarations both bodies keep.
        let mut prologue: Vec<Stmt> = Vec::new();
        let mut kept: Vec<Stmt> = Vec::new();
        if let (Some(sig), true) = (sig, plain_prologue) {
            for (i, p) in f.params.iter().enumerate() {
                let Some(Some(r)) = sig.record_params.get(i) else {
                    continue;
                };
                if !self.eligible.contains(&p.id) || self.recs.contains_key(&p.id) {
                    continue;
                }
                let rec = self.new_rec(r.tmpl, r.opt);
                self.unpack_param(&rec, p.id, &p.name, &mut prologue);
                kept.extend(self.scratch_stmts(&rec, &p.name));
                lanes[i] = Some(lanes_of(&rec));
                self.recs.insert(p.id, rec);
                self.census.params += 1;
            }
        }
        let saved_ret = self.ret.take();
        if let Some(r) = sig
            .filter(|_| plain_prologue)
            .and_then(|s| s.record_ret)
            .filter(|r| self.shapes.contains_key(&r.tmpl))
        {
            // The result lanes always track presence: `return;` and a
            // completion without `return` are the absent record.
            let rec = self.new_rec(r.tmpl, true);
            kept.extend(self.decl_stmts(&rec, "return"));
            let id = self.fresh();
            self.recs.insert(id, rec);
            self.ret = Some(id);
            self.census.returns += 1;
        }
        self.stmts(&mut f.body);
        let ret = self.ret.is_some();
        self.ret = saved_ret;
        if lanes.iter().any(Option::is_some) || ret {
            let len = prologue.len();
            prologue.append(&mut kept);
            f.body.splice(at..at, prologue);
            self.abi.fns.insert(
                f.id,
                FactsRecordFn {
                    prologue_at: at,
                    prologue_len: len,
                    params: lanes,
                    ret,
                },
            );
        }
    }

    /// A record of `tmpl` with fresh locals.
    fn new_rec(&mut self, tmpl: u32, track: bool) -> Rec {
        let n = self.shapes[&tmpl].keys.len();
        Rec {
            tmpl,
            track,
            present: self.fresh(),
            fields: (0..n).map(|_| self.fresh()).collect(),
            tmp: self.fresh(),
            next: (0..n).map(|_| self.fresh()).collect(),
            agg: self.fresh(),
        }
    }

    /// A lane's value while the record is absent: a Number lane holds a
    /// Number at all times.
    fn filler(&self, tmpl: u32, i: usize) -> Expr {
        if self.shapes[&tmpl].numeric.get(i) == Some(&true) {
            Expr::Number(0.0)
        } else {
            undefined()
        }
    }

    /// The scratch locals an assignment to `rec` uses.
    fn scratch_stmts(&self, rec: &Rec, name: &str) -> Vec<Stmt> {
        let keys = &self.shapes[&rec.tmpl].keys;
        let mut out = vec![
            let_stmt(
                rec.tmp,
                format!("{name}$boxed"),
                Type::Any,
                true,
                undefined(),
            ),
            let_stmt(rec.agg, format!("{name}$agg"), Type::Any, true, undefined()),
        ];
        for (i, k) in keys.iter().enumerate() {
            out.push(let_stmt(
                rec.next[i],
                format!("{name}${k}$next"),
                Type::Any,
                true,
                undefined(),
            ));
        }
        out
    }

    /// The declarations of an absent `rec`: presence, fields, scratch.
    fn decl_stmts(&self, rec: &Rec, name: &str) -> Vec<Stmt> {
        let keys = &self.shapes[&rec.tmpl].keys;
        let mut out = Vec::new();
        if rec.track {
            out.push(let_stmt(
                rec.present,
                format!("{name}$present"),
                Type::Any,
                true,
                Expr::Bool(false),
            ));
        }
        for (i, k) in keys.iter().enumerate() {
            out.push(let_stmt(
                rec.fields[i],
                format!("{name}${k}"),
                Type::Any,
                true,
                self.filler(rec.tmpl, i),
            ));
        }
        out.extend(self.scratch_stmts(rec, name));
        out
    }

    /// The entry prologue of record parameter `p`: its lanes read from the
    /// parameter key by key (`F$sig` receives them as parameters instead).
    fn unpack_param(&self, rec: &Rec, p: LocalId, name: &str, out: &mut Vec<Stmt>) {
        let keys = &self.shapes[&rec.tmpl].keys;
        if rec.track {
            out.push(let_stmt(
                rec.present,
                format!("{name}$present"),
                Type::Any,
                true,
                Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(Expr::LocalGet(p)),
                    right: Box::new(undefined()),
                },
            ));
        }
        for (i, k) in keys.iter().enumerate() {
            let read = Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(p)),
                property: k.clone(),
                byte_offset: 0,
                byte_end: 0,
            };
            let v = if rec.track {
                Expr::Conditional {
                    condition: Box::new(Expr::LocalGet(rec.present)),
                    then_expr: Box::new(read),
                    else_expr: Box::new(self.filler(rec.tmpl, i)),
                }
            } else {
                read
            };
            out.push(let_stmt(
                rec.fields[i],
                format!("{name}${k}"),
                Type::Any,
                true,
                v,
            ));
        }
    }

    /// A temporary record of `tmpl`, declared before the statement in
    /// progress.
    fn temp(&mut self, tmpl: u32) -> LocalId {
        let rec = self.new_rec(tmpl, true);
        let id = self.fresh();
        let decls = self.decl_stmts(&rec, &format!("rec${id}"));
        self.pending.extend(decls);
        self.recs.insert(id, rec);
        id
    }

    /// The signature of the typed call `e` when a slot of it is a record
    /// (`typed_calls` names the call's offset; a direct call names its
    /// callee).
    fn record_call(&self, e: &Expr) -> Option<&'_ FactsSignature> {
        let Expr::Call {
            callee,
            byte_offset,
            ..
        } = e
        else {
            return None;
        };
        let (offset, direct) = match callee.as_ref() {
            Expr::FuncRef(fid) => (*byte_offset, Some(*fid)),
            Expr::PropertyGet {
                byte_offset: member,
                ..
            } => (if *member != 0 { *member } else { *byte_offset }, None),
            _ => return None,
        };
        if offset == 0 {
            return None;
        }
        let facts: &ElisionFacts = self.facts;
        let fid = *facts.typed_calls.get(&offset)?;
        if direct.is_some_and(|d| d != fid) || !self.lane_fns.contains(&fid) {
            return None;
        }
        facts.signatures.get(&fid).filter(|s| has_record(s))
    }

    /// The record a typed call `e` returns.
    fn record_ret_call(&self, e: &Expr) -> Option<FactsRecRep> {
        // VR-4b: `a.pop()` and `a[i]` of a record array return its record,
        // absent when the array is empty or the index out of bounds.
        if let Some(tmpl) = self.ra_source(e) {
            return Some(FactsRecRep { tmpl, opt: true });
        }
        self.record_call(e)?
            .record_ret
            .filter(|r| self.shapes.contains_key(&r.tmpl))
    }

    /// Typed call `e` with a record slot: each argument in a record slot is
    /// the copy of a binding's lanes (a value that is no binding is first
    /// written into a temporary's), the other operands are rewritten as
    /// usual.
    fn call_operands(&mut self, e: &mut Expr) {
        if let Some(tmpl) = self.ra_source(e) {
            self.ra_lower_source(e, tmpl);
            return;
        }
        let slots: Vec<Option<FactsRecRep>> = self
            .record_call(e)
            .map(|s| s.record_params.clone())
            .unwrap_or_default();
        let Expr::Call { callee, args, .. } = e else {
            return;
        };
        self.expr(callee);
        for (i, a) in args.iter_mut().enumerate() {
            let slot = slots
                .get(i)
                .copied()
                .flatten()
                .filter(|r| self.shapes.contains_key(&r.tmpl));
            let Some(r) = slot else {
                self.expr(a);
                continue;
            };
            if is_undefined(a) {
                continue;
            }
            self.census.passed += 1;
            if let Some(u) = self.rec_of(a).filter(|u| self.recs[u].tmpl == r.tmpl) {
                self.reach(u);
                *a = self.copy(u);
                continue;
            }
            let t = self.temp(r.tmpl);
            let v = std::mem::replace(a, undefined());
            let list = self.assign(t, v);
            *a = seq(list, self.copy(t));
        }
    }

    /// Bind the record result of typed call `call` to `u`'s lanes: the
    /// call's value goes to `u`'s aggregate temporary, which the typed call
    /// leaves `true` (present, the lanes written) or `undefined`; a boxed
    /// result is an object, read key by key.
    fn call_into(&mut self, u: LocalId, mut call: Expr) -> Vec<Expr> {
        // VR-4b: a record array's element read is absent only where the
        // facts say the destination may be (`a[i]` under the for-of's bound
        // never is).
        let element = self.ra_source(&call).is_some();
        self.call_operands(&mut call);
        self.census.calls += 1;
        let rec = self.recs[&u].clone();
        let opt = if element {
            rec.track
        } else {
            self.record_ret_call(&call).is_none_or(|r| r.opt)
        };
        if opt && !rec.track {
            self.failed.insert(u);
        }
        self.abi.aggs.insert(rec.agg, lanes_of(&rec));
        let keys = self.shapes[&rec.tmpl].keys.clone();
        let agg = || Box::new(Expr::LocalGet(rec.agg));
        let mut list = vec![Expr::LocalSet(rec.agg, Box::new(call))];
        if rec.track {
            list.push(Expr::LocalSet(
                rec.present,
                Box::new(Expr::Compare {
                    op: CompareOp::Ne,
                    left: agg(),
                    right: Box::new(undefined()),
                }),
            ));
        }
        let mut reads = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let read = Expr::PropertyGet {
                object: agg(),
                property: k.clone(),
                byte_offset: 0,
                byte_end: 0,
            };
            let v = if rec.track {
                Expr::Conditional {
                    condition: Box::new(Expr::LocalGet(rec.present)),
                    then_expr: Box::new(read),
                    else_expr: Box::new(self.filler(rec.tmpl, i)),
                }
            } else {
                read
            };
            reads.push(Expr::LocalSet(rec.fields[i], Box::new(v)));
        }
        list.push(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Eq,
                left: agg(),
                right: Box::new(Expr::Bool(true)),
            }),
            then_expr: Box::new(undefined()),
            else_expr: Box::new(seq(reads, undefined())),
        });
        list.push(Expr::LocalSet(rec.agg, Box::new(undefined())));
        list
    }

    /// The copy of `u`'s lanes in a record slot or a record return: the
    /// materializer's expression, which `record_sig::copy_lanes` reads back.
    fn copy(&mut self, u: LocalId) -> Expr {
        let e = self.materialize(u);
        self.census.materialized -= 1;
        e
    }

    fn stmts(&mut self, list: &mut Vec<Stmt>) {
        let old = std::mem::take(list);
        let outer = std::mem::take(&mut self.pending);
        self.stmts_inner(old, list);
        self.pending = outer;
    }

    fn stmts_inner(&mut self, old: Vec<Stmt>, list: &mut Vec<Stmt>) {
        for mut s in old {
            if let Stmt::Let {
                id,
                name,
                mutable,
                init,
                ..
            } = &mut s
            {
                if self.eligible.contains(id) {
                    if self.recs.contains_key(id) {
                        self.failed.insert(*id);
                    } else {
                        let (id, name, mutable, init) = (*id, name.clone(), *mutable, init.take());
                        let mut out = Vec::new();
                        self.declare(id, &name, mutable, init, &mut out);
                        list.append(&mut self.pending);
                        list.append(&mut out);
                        continue;
                    }
                }
            }
            self.stmt(&mut s);
            list.append(&mut self.pending);
            list.push(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { id, init, ty, .. } => {
                // VR-4b: a record array is no Array to the lowering.
                if self.ra_locals.contains_key(id) {
                    *ty = Type::Any;
                }
                if let (Some(tmpl), Some(e)) = (self.facts.record_array_birth_locals.get(id), &mut *init) {
                    if is_empty_array(e) {
                        *e = self.ra_new(*tmpl);
                    }
                }
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) => self.discarded(e),
            // VR-3b: a record-returning body writes the value into its
            // result lanes and returns their copy.
            Stmt::Return(Some(e)) if self.ret.is_some() => {
                let ret = self.ret.expect("result lanes");
                let v = std::mem::replace(e, undefined());
                let list = self.assign(ret, v);
                *e = seq(list, self.copy(ret));
            }
            Stmt::Return(Some(e)) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                // VR-4b: a for-of over a record array is its index arm; the
                // iterator arm never runs for one.
                if self.ra_for_of(condition) {
                    *condition = Expr::Bool(true);
                    *else_branch = None;
                    self.census.array_ops += 1;
                }
                self.cond(condition);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } => {
                self.cond(condition);
                self.stmts(body);
            }
            Stmt::DoWhile { body, condition } => {
                self.stmts(body);
                self.cond(condition);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = condition {
                    self.cond(c);
                }
                if let Some(u) = update {
                    self.discarded(u);
                }
                self.stmts(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    self.stmts(&mut c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(t) = &mut c.test {
                        self.expr(t);
                    }
                    self.stmts(&mut c.body);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    /// The `new`/object literal of `tmpl`'s birth in `e`: its values in
    /// layout order, when `e` is one.
    fn birth_values(&self, tmpl: u32, e: &Expr) -> Option<Vec<(usize, Expr)>> {
        let shape = &self.shapes[&tmpl];
        let pos = |k: &str| shape.keys.iter().position(|x| x == k);
        match (&shape.birth, e) {
            (
                Birth::Class { name, fields },
                Expr::New {
                    class_name, args, ..
                },
            ) if class_name == name && args.len() == fields.len() => fields
                .iter()
                .zip(args)
                .map(|(f, a)| Some((pos(f)?, a.clone())))
                .collect(),
            (Birth::Object, Expr::Object(props))
                if props.len() == shape.keys.len()
                    && props.iter().zip(&shape.keys).all(|((k, _), x)| k == x) =>
            {
                Some(
                    props
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(i, (_, v))| (i, v))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    /// Replace `let u = init` by the field and presence locals of `u`.
    fn declare(
        &mut self,
        id: LocalId,
        name: &str,
        mutable: bool,
        init: Option<Expr>,
        out: &mut Vec<Stmt>,
    ) {
        let r = self.cands[&id];
        let keys = self.shapes[&r.tmpl].keys.clone();
        let rec = self.new_rec(r.tmpl, r.opt || init.is_none());
        let field_name = |k: &str| format!("{name}${k}");
        let direct = init.as_ref().and_then(|e| self.birth_values(r.tmpl, e));
        match direct {
            // The literal's values, in their evaluation order.
            Some(mut values) => {
                for (_, v) in values.iter_mut() {
                    self.expr(v);
                }
                if rec.track {
                    out.push(let_stmt(
                        rec.present,
                        format!("{name}$present"),
                        Type::Any,
                        true,
                        Expr::Bool(true),
                    ));
                }
                for (i, v) in values {
                    out.push(let_stmt(
                        rec.fields[i],
                        field_name(&keys[i]),
                        Type::Any,
                        mutable,
                        v,
                    ));
                }
                if mutable {
                    out.extend(self.scratch_stmts(&rec, name));
                }
                self.recs.insert(id, rec);
            }
            None => {
                out.extend(self.decl_stmts(&rec, name));
                self.recs.insert(id, rec);
                if let Some(e) = init {
                    let list = self.assign(id, e);
                    out.push(Stmt::Expr(seq(list, undefined())));
                }
            }
        }
    }

    /// The expressions that evaluate record value `e` into `u`'s locals.
    fn assign(&mut self, u: LocalId, mut e: Expr) -> Vec<Expr> {
        let rec = self.recs[&u].clone();
        // VR-3b: a typed call that returns this record.
        if self.record_ret_call(&e).is_some_and(|r| r.tmpl == rec.tmpl) {
            return self.call_into(u, e);
        }
        // A literal birth: its values in their evaluation order. When one
        // reads `u`, every value is taken before the first field is written.
        if let Some(values) = self.birth_values(rec.tmpl, &e) {
            let reads_u = values.iter().any(|(_, v)| refers_to(v, u));
            let mut list = Vec::new();
            let mut moves = Vec::new();
            for (i, mut v) in values {
                self.expr(&mut v);
                if reads_u {
                    list.push(Expr::LocalSet(rec.next[i], Box::new(v)));
                    moves.push(Expr::LocalSet(
                        rec.fields[i],
                        Box::new(Expr::LocalGet(rec.next[i])),
                    ));
                } else {
                    list.push(Expr::LocalSet(rec.fields[i], Box::new(v)));
                }
            }
            list.extend(moves);
            if rec.track {
                list.push(Expr::LocalSet(rec.present, Box::new(Expr::Bool(true))));
            }
            return list;
        }
        if let Some(v) = self.rec_of(&e) {
            if self.recs[&v].tmpl == rec.tmpl {
                self.reach(v);
                if v == u {
                    return Vec::new();
                }
                let src = self.recs[&v].clone();
                let mut list: Vec<Expr> = rec
                    .fields
                    .iter()
                    .zip(&src.fields)
                    .map(|(d, s)| Expr::LocalSet(*d, Box::new(Expr::LocalGet(*s))))
                    .collect();
                if rec.track {
                    list.push(Expr::LocalSet(
                        rec.present,
                        Box::new(self.presence_of(&src)),
                    ));
                } else if src.track {
                    self.failed.insert(u);
                }
                return list;
            }
        }
        if is_undefined(&e) {
            if !rec.track {
                self.failed.insert(u);
            }
            return vec![Expr::LocalSet(rec.present, Box::new(Expr::Bool(false)))];
        }
        if let Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } = e
        {
            let mut c = *condition;
            self.cond(&mut c);
            let a = self.assign(u, *then_expr);
            let b = self.assign(u, *else_expr);
            return vec![Expr::Conditional {
                condition: Box::new(c),
                then_expr: Box::new(seq(a, undefined())),
                else_expr: Box::new(seq(b, undefined())),
            }];
        }
        // A value held boxed: read key by key.
        self.expr(&mut e);
        self.census.unpacked += 1;
        let keys = self.shapes[&rec.tmpl].keys.clone();
        let tmp = || Box::new(Expr::LocalGet(rec.tmp));
        let mut list = vec![Expr::LocalSet(rec.tmp, Box::new(e))];
        if rec.track {
            list.push(Expr::LocalSet(
                rec.present,
                Box::new(Expr::Compare {
                    op: CompareOp::Ne,
                    left: tmp(),
                    right: Box::new(undefined()),
                }),
            ));
        }
        for (i, k) in keys.iter().enumerate() {
            let read = Expr::PropertyGet {
                object: tmp(),
                property: k.clone(),
                byte_offset: 0,
                byte_end: 0,
            };
            let v = if rec.track {
                Expr::Conditional {
                    condition: Box::new(Expr::LocalGet(rec.present)),
                    then_expr: Box::new(read),
                    else_expr: Box::new(self.filler(rec.tmpl, i)),
                }
            } else {
                read
            };
            list.push(Expr::LocalSet(rec.fields[i], Box::new(v)));
        }
        list.push(Expr::LocalSet(rec.tmp, Box::new(undefined())));
        list
    }

    fn presence_of(&self, rec: &Rec) -> Expr {
        if rec.track {
            Expr::LocalGet(rec.present)
        } else {
            Expr::Bool(true)
        }
    }

    /// A fresh object of `u`'s template from its fields (§4.2), or
    /// `undefined` when it is absent.
    fn materialize(&mut self, u: LocalId) -> Expr {
        self.census.materialized += 1;
        let rec = self.recs[&u].clone();
        let shape = &self.shapes[&rec.tmpl];
        let field = |k: &str| {
            let i = shape
                .keys
                .iter()
                .position(|x| x == k)
                .expect("a layout key");
            Expr::LocalGet(rec.fields[i])
        };
        let born = match &shape.birth {
            Birth::Class { name, fields } => Expr::New {
                class_name: name.clone(),
                args: fields.iter().map(|f| field(f)).collect(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
                byte_end: 0,
            },
            Birth::Object => {
                Expr::Object(shape.keys.iter().map(|k| (k.clone(), field(k))).collect())
            }
        };
        if rec.track {
            Expr::Conditional {
                condition: Box::new(Expr::LocalGet(rec.present)),
                then_expr: Box::new(born),
                else_expr: Box::new(undefined()),
            }
        } else {
            born
        }
    }

    /// A truthiness position: a record is truthy exactly when present.
    /// VR-4b: the record template of the record array `e` holds, when `e`
    /// is a typed receiver (a binding, a named read).
    fn ra_of(&self, e: &Expr) -> Option<u32> {
        match e {
            Expr::LocalGet(id) => self.ra_locals.get(id).copied(),
            Expr::PropertyGet {
                property,
                byte_offset,
                ..
            } if *byte_offset != 0 => self
                .facts
                .record_array_reads
                .get(&(*byte_offset, property.clone()))
                .copied()
                .filter(|t| self.shapes.contains_key(t)),
            _ => None,
        }
    }

    /// VR-4b: `e` is `a.pop()` or `a[i]` of a record array: its record.
    fn ra_source(&self, e: &Expr) -> Option<u32> {
        match e {
            Expr::ArrayPop(id) => self.ra_locals.get(id).copied(),
            Expr::Call { callee, args, .. } if args.is_empty() => match callee.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } if property == "pop" => self.ra_of(object),
                _ => None,
            },
            Expr::IndexGet { object, .. } => self.ra_of(object),
            _ => None,
        }
    }

    /// VR-4b: `a.pop()` / `a[i]` as its intrinsic, operands rewritten.
    fn ra_lower_source(&mut self, e: &mut Expr, tmpl: u32) {
        let t = Expr::Integer(i64::from(tmpl));
        *e = match std::mem::replace(e, undefined()) {
            Expr::ArrayPop(id) => rt(RA_POP, vec![Expr::LocalGet(id), t]),
            Expr::Call { callee, .. } => {
                let Expr::PropertyGet { object, .. } = *callee else {
                    unreachable!("a record array's pop")
                };
                let mut o = *object;
                self.expr(&mut o);
                rt(RA_POP, vec![o, t])
            }
            Expr::IndexGet { object, index, .. } => {
                let (mut o, mut i) = (*object, *index);
                self.expr(&mut o);
                self.expr(&mut i);
                rt(RA_GET, vec![o, t, i])
            }
            _ => unreachable!("a record array's source"),
        };
        self.census.array_ops += 1;
    }

    /// VR-4b: the birth of a record array of `tmpl`.
    fn ra_new(&mut self, tmpl: u32) -> Expr {
        assert!(
            self.shapes.contains_key(&tmpl),
            "ICE: value record array: the facts give no layout for record template {tmpl} in this module"
        );
        self.census.arrays += 1;
        rt(RA_NEW, vec![Expr::Integer(i64::from(tmpl))])
    }

    /// VR-4b: `a.push(v)`: the copy of `v`'s lanes is stored.
    fn ra_push(&mut self, recv: Expr, tmpl: u32, v: Expr) -> Expr {
        self.census.passed += 1;
        self.census.array_ops += 1;
        let copy = match self.rec_of(&v).filter(|u| self.recs[u].tmpl == tmpl) {
            Some(u) => {
                self.reach(u);
                self.copy(u)
            }
            None => {
                let t = self.temp(tmpl);
                let list = self.assign(t, v);
                seq(list, self.copy(t))
            }
        };
        rt(RA_PUSH, vec![recv, Expr::Integer(i64::from(tmpl)), copy])
    }

    /// VR-4b: the guard of HIR's for-of
    /// (`arrayRecordNeedsIterator(src) === false`) over a record array.
    fn ra_for_of(&self, condition: &Expr) -> bool {
        let Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } = condition
        else {
            return false;
        };
        let Expr::NativeMethodCall {
            module,
            method,
            args,
            object: None,
            ..
        } = left.as_ref()
        else {
            return false;
        };
        module == RT
            && method == "arrayRecordNeedsIterator"
            && matches!(right.as_ref(), Expr::Bool(false))
            && matches!(args.as_slice(), [a] if self.ra_of(a).is_some())
    }

    /// VR-4b: `e` is a record array's birth in an expression, its `push`
    /// or its `length`: rewritten to the intrinsic. Any other operation on
    /// a typed receiver is a compile error (module docs).
    fn ra_expr(&mut self, e: &mut Expr) -> bool {
        if self.ra_locals.is_empty() && !self.facts.has_record_arrays() {
            return false;
        }
        match e {
            Expr::LocalSet(id, v)
                if is_empty_array(v) && self.facts.record_array_birth_locals.contains_key(id) =>
            {
                **v = self.ra_new(self.facts.record_array_birth_locals[id]);
                return true;
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                element_offset,
                ..
            } if is_empty_array(value)
                && self
                    .facts
                    .record_array_birth_stores
                    .contains_key(element_offset) =>
            {
                **value = self.ra_new(self.facts.record_array_birth_stores[element_offset]);
                self.expr(target);
                self.expr(key);
                self.expr(receiver);
                return true;
            }
            _ => {}
        }
        // `push`, in each form HIR lowering gives it.
        let pushed: Option<(u32, usize)> = match &*e {
            Expr::ArrayPush { array_id, .. } => self.ra_locals.get(array_id).map(|t| (*t, 1)),
            Expr::NativeMethodCall {
                module,
                method,
                object: Some(o),
                args,
                ..
            } if module == "array" && matches!(method.as_str(), "push_single" | "push") => {
                self.ra_of(o).map(|t| (t, args.len()))
            }
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } if property == "push" => self.ra_of(object).map(|t| (t, args.len())),
                _ => None,
            },
            _ => None,
        };
        if let Some((tmpl, n)) = pushed {
            if n != 1 {
                panic!("ICE: value record array: push of {n} values is no record-array intrinsic");
            }
            let (mut recv, v) = match std::mem::replace(e, undefined()) {
                Expr::ArrayPush {
                    array_id, value, ..
                } => (Expr::LocalGet(array_id), *value),
                Expr::NativeMethodCall {
                    object: Some(o),
                    mut args,
                    ..
                } => (*o, args.remove(0)),
                Expr::Call {
                    callee, mut args, ..
                } => {
                    let Expr::PropertyGet { object, .. } = *callee else {
                        unreachable!("a record array's push")
                    };
                    (*object, args.remove(0))
                }
                _ => unreachable!("a record array's push"),
            };
            self.expr(&mut recv);
            *e = self.ra_push(recv, tmpl, v);
            return true;
        }
        if let Expr::PropertyGet {
            object, property, ..
        } = e
        {
            if property == "length" && self.ra_of(object).is_some() {
                let mut o = std::mem::replace(object.as_mut(), undefined());
                self.expr(&mut o);
                *e = rt(RA_LENGTH, vec![o]);
                self.census.array_ops += 1;
                return true;
            }
        }
        if let Some(what) = self.ra_misuse(e) {
            panic!("ICE: value record array: {what} is no record-array intrinsic");
        }
        false
    }

    /// VR-4b: an operation on a typed receiver that no intrinsic covers.
    fn ra_misuse(&self, e: &Expr) -> Option<String> {
        let on = |o: &Expr| self.ra_of(o).is_some();
        match e {
            Expr::PropertyGet {
                object, property, ..
            } if on(object) => Some(format!("a read of `{property}`")),
            Expr::PropertySet {
                object, property, ..
            } if on(object) => Some(format!("a store of `{property}`")),
            Expr::IndexSet { object, .. } if on(object) => Some("an index store".into()),
            Expr::PutValueSet { target, .. } if on(target) => Some("a store".into()),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } if on(object) => Some(format!("a call of `{property}`")),
                _ => None,
            },
            Expr::NativeMethodCall {
                module,
                method,
                object,
                args,
                ..
            } if module != RT || !is_intrinsic(method) => {
                (object.as_deref().is_some_and(on) || args.iter().any(on))
                    .then(|| format!("the native `{module}.{method}`"))
            }
            Expr::ArrayPop(id)
            | Expr::ArrayShift(id)
            | Expr::ArrayPush { array_id: id, .. }
            | Expr::ArrayPushSpread { array_id: id, .. }
                if self.ra_locals.contains_key(id) =>
            {
                Some("an Array operation".into())
            }
            Expr::GetIterator(o) if on(o) => Some("an iteration".into()),
            _ => None,
        }
    }

    fn cond(&mut self, e: &mut Expr) {
        if let Some(u) = self.rec_of(e) {
            self.reach(u);
            self.census.presence += 1;
            *e = self.presence_of(&self.recs[&u].clone());
            return;
        }
        self.expr(e);
    }

    /// An expression whose value is discarded: an assignment to a record
    /// binding writes its fields and takes no copy.
    fn discarded(&mut self, e: &mut Expr) {
        match e {
            Expr::LocalSet(u, v) if self.recs.contains_key(u) => {
                let u = *u;
                self.reach(u);
                let v = std::mem::replace(v.as_mut(), undefined());
                let list = self.assign(u, v);
                *e = seq(list, undefined());
            }
            Expr::Sequence(list) => {
                for x in list.iter_mut() {
                    self.discarded(x);
                }
            }
            _ if self.record_ret_call(e).is_some() => {
                let r = self.record_ret_call(e).expect("a record result");
                let t = self.temp(r.tmpl);
                let call = std::mem::replace(e, undefined());
                let list = self.call_into(t, call);
                *e = seq(list, undefined());
            }
            _ => self.expr(e),
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Some(u) = self.rec_of(e) {
            self.reach(u);
            *e = self.materialize(u);
            return;
        }
        // VR-3b: a typed call that returns a record, in a value position:
        // its lanes go to a temporary, whose copy is the value.
        if let Some(r) = self.record_ret_call(e) {
            let t = self.temp(r.tmpl);
            let call = std::mem::replace(e, undefined());
            let list = self.call_into(t, call);
            *e = seq(list, self.materialize(t));
            return;
        }
        if self.record_call(e).is_some() {
            self.call_operands(e);
            return;
        }
        if self.ra_expr(e) {
            return;
        }
        match e {
            Expr::LocalSet(u, v) if self.recs.contains_key(u) => {
                let u = *u;
                self.reach(u);
                let v = std::mem::replace(v.as_mut(), undefined());
                let list = self.assign(u, v);
                let value = self.materialize(u);
                *e = seq(list, value);
            }
            // A field of a typed call's record result: no copy.
            Expr::PropertyGet {
                object, property, ..
            } if self
                .record_ret_call(object)
                .is_some_and(|r| self.shapes[&r.tmpl].keys.iter().any(|k| k == property)) =>
            {
                let r = self.record_ret_call(object).expect("a record result");
                let t = self.temp(r.tmpl);
                let call = std::mem::replace(object.as_mut(), Expr::LocalGet(t));
                let list = self.call_into(t, call);
                // `t.k`, the field read of the temporary.
                let mut read = std::mem::replace(e, undefined());
                self.expr(&mut read);
                *e = seq(list, read);
            }
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                byte_end,
            } if self.rec_of(object).is_some() => {
                let u = self.rec_of(object).unwrap();
                let rec = self.recs[&u].clone();
                match self.shapes[&rec.tmpl]
                    .keys
                    .iter()
                    .position(|k| k == property)
                {
                    Some(i) => {
                        self.reach(u);
                        self.census.field_reads += 1;
                        let read = Expr::LocalGet(rec.fields[i]);
                        let value = if rec.track {
                            // Absent: the read throws as on `undefined`.
                            let throw = Expr::PropertyGet {
                                object: Box::new(undefined()),
                                property: property.clone(),
                                byte_offset: *byte_offset,
                                byte_end: *byte_end,
                            };
                            Expr::Conditional {
                                condition: Box::new(Expr::LocalGet(rec.present)),
                                then_expr: Box::new(read),
                                else_expr: Box::new(throw),
                            }
                        } else {
                            read
                        };
                        *e = value;
                    }
                    None => self.expr(object),
                }
            }
            Expr::Compare { op, left, right }
                if matches!(
                    op,
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                ) && (self.rec_of(left).is_some() || self.rec_of(right).is_some()) =>
            {
                let (r, other) = if self.rec_of(left).is_some() {
                    (left.as_ref(), right.as_ref())
                } else {
                    (right.as_ref(), left.as_ref())
                };
                let loose = matches!(op, CompareOp::LooseEq | CompareOp::LooseNe);
                let eq = matches!(op, CompareOp::Eq | CompareOp::LooseEq);
                let absent_test = is_undefined(other) || (loose && matches!(other, Expr::Null));
                let never = !loose && matches!(other, Expr::Null);
                if absent_test || never {
                    let u = self.rec_of(r).unwrap();
                    self.reach(u);
                    self.census.presence += 1;
                    let rec = self.recs[&u].clone();
                    let absent = if never {
                        Expr::Bool(false)
                    } else if rec.track {
                        Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(Expr::LocalGet(rec.present)),
                        }
                    } else {
                        Expr::Bool(false)
                    };
                    *e = if eq {
                        absent
                    } else {
                        Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(absent),
                        }
                    };
                } else {
                    self.expr(left);
                    self.expr(right);
                }
            }
            Expr::TypeOf(inner) if self.rec_of(inner).is_some() => {
                let u = self.rec_of(inner).unwrap();
                self.reach(u);
                self.census.presence += 1;
                let rec = self.recs[&u].clone();
                let object = Expr::String("object".into());
                *e = if rec.track {
                    Expr::Conditional {
                        condition: Box::new(Expr::LocalGet(rec.present)),
                        then_expr: Box::new(object),
                        else_expr: Box::new(Expr::String("undefined".into())),
                    }
                } else {
                    object
                };
            }
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => self.cond(operand),
            Expr::Logical { op, left, right } if self.rec_of(left).is_some() => {
                let u = self.rec_of(left).unwrap();
                self.reach(u);
                self.census.presence += 1;
                let present = self.presence_of(&self.recs[&u].clone());
                let mut r = std::mem::replace(right.as_mut(), undefined());
                self.expr(&mut r);
                *e = match op {
                    LogicalOp::And => Expr::Conditional {
                        condition: Box::new(present),
                        then_expr: Box::new(r),
                        else_expr: Box::new(undefined()),
                    },
                    LogicalOp::Or | LogicalOp::Coalesce => Expr::Conditional {
                        condition: Box::new(present),
                        then_expr: Box::new(self.materialize(u)),
                        else_expr: Box::new(r),
                    },
                };
            }
            Expr::Logical { left, right, .. } => {
                self.cond_value(left);
                self.expr(right);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.cond(condition);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            Expr::Closure { params, body, .. } => {
                for p in params {
                    if let Some(d) = &mut p.default {
                        self.expr(d);
                    }
                }
                // A closure's `return` is its own.
                let ret = self.ret.take();
                self.stmts(body);
                self.ret = ret;
            }
            _ => perry_hir::walker::walk_expr_children_mut(e, &mut |c| self.expr(c)),
        }
    }

    /// The left operand of `&&`/`||`/`??` that is no record binding itself:
    /// a value position.
    fn cond_value(&mut self, e: &mut Expr) {
        self.expr(e);
    }
}

/// The functions whose record slots cross as lanes: a signature with a
/// record slot, and every typed call of the function is one the rewrite
/// writes in lane form and codegen enters `F$sig` from
/// (`record_sig::try_lower_call`): a direct call `F(…)` of a plain function,
/// or a method call `o.m(…)` of a class's instance method. A function with a
/// typed call of any other form (an object literal's method reached as
/// `o.m(…)`, a static method, a call node that is no `Expr::Call`) keeps
/// boxed slots: that call enters its boxed entry, which could not take a
/// record.
fn lane_functions(hir: &Module, facts: &ElisionFacts) -> HashSet<u32> {
    let plain: HashSet<u32> = hir.functions.iter().map(|f| f.id).collect();
    let methods: HashSet<u32> = hir
        .classes
        .iter()
        .flat_map(|c| c.methods.iter().map(|m| m.id))
        .collect();
    // Per typed call offset: the call forms found there.
    let mut direct: HashMap<u32, HashSet<u32>> = HashMap::new();
    let mut member: HashSet<u32> = HashSet::new();
    perry_hir::tdz_check::for_each_module_expr(hir, &mut |e| {
        if let Expr::Call {
            callee,
            byte_offset,
            ..
        } = e
        {
            match callee.as_ref() {
                Expr::FuncRef(fid) => {
                    direct.entry(*byte_offset).or_default().insert(*fid);
                }
                Expr::PropertyGet {
                    byte_offset: at, ..
                } => {
                    member.insert(if *at != 0 { *at } else { *byte_offset });
                }
                _ => {}
            }
        }
    });
    let mut out: HashSet<u32> = facts
        .signatures
        .iter()
        .filter(|(id, sig)| has_record(sig) && (plain.contains(id) || methods.contains(id)))
        .map(|(id, _)| *id)
        .collect();
    for (offset, fid) in &facts.typed_calls {
        if !out.contains(fid) {
            continue;
        }
        let ok = if plain.contains(fid) {
            !member.contains(offset)
                && direct
                    .get(offset)
                    .is_some_and(|fs| fs.len() == 1 && fs.contains(fid))
        } else {
            member.contains(offset) && !direct.contains_key(offset)
        };
        if !ok {
            out.remove(fid);
        }
    }
    out
}

/// A parameter or the result of `sig` is a value record.
pub(crate) fn has_record(sig: &FactsSignature) -> bool {
    sig.record_ret.is_some() || sig.record_params.iter().any(Option::is_some)
}

fn lanes_of(rec: &Rec) -> FactsRecordLanes {
    FactsRecordLanes {
        tmpl: rec.tmpl,
        present: rec.track.then_some(rec.present),
        fields: rec.fields.clone(),
    }
}

/// A statement of a default prologue names a local `pred` holds for.
fn stmt_refers_to_any(s: &Stmt, pred: &dyn Fn(LocalId) -> bool) -> bool {
    let mut found = false;
    let mut on = |x: &Expr| {
        if matches!(x, Expr::LocalGet(id) | Expr::LocalSet(id, _) if pred(*id)) {
            found = true;
        }
    };
    if let Stmt::If {
        condition,
        then_branch,
        ..
    } = s
    {
        perry_hir::tdz_check::for_each_expr_in(condition, &mut on);
        for t in then_branch {
            if let Stmt::Expr(e) = t {
                perry_hir::tdz_check::for_each_expr_in(e, &mut on);
            }
        }
    }
    found
}

impl Shape {
    /// How codegen reads a copy of this template back
    /// (`record_sig::copy_lanes`).
    fn birth_fact(&self) -> crate::codegen::FactsRecordBirth {
        match &self.birth {
            Birth::Class { name, fields } => crate::codegen::FactsRecordBirth {
                class: Some(name.clone()),
                order: fields
                    .iter()
                    .map(|f| self.keys.iter().position(|k| k == f).expect("a layout key"))
                    .collect(),
            },
            Birth::Object => crate::codegen::FactsRecordBirth {
                class: None,
                order: (0..self.keys.len()).collect(),
            },
        }
    }
}

/// `e` reads or writes local `u`.
fn refers_to(e: &Expr, u: LocalId) -> bool {
    let mut found = false;
    perry_hir::tdz_check::for_each_expr_in(e, &mut |x| {
        if matches!(x, Expr::LocalGet(id) | Expr::LocalSet(id, _) if *id == u) {
            found = true;
        }
    });
    found
}

#[cfg(test)]
#[path = "value_records_tests.rs"]
mod tests;
