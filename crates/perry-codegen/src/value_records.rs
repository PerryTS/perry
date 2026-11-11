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
//! Records do not cross program calls yet (wpfacts `RECORD_SIGNATURES`,
//! VR-3b), and records in arrays are VR-4b.
use std::collections::{HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::{CompareOp, Expr, Function, LogicalOp, Module, Stmt, UnaryOp};

use crate::codegen::{ElisionFacts, FactsRecRep};

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
}

/// The module with every eligible record binding rewritten, or `None`
/// when there is none (the module lowers as it is).
pub(crate) fn rewrite(hir: &Module, facts: &ElisionFacts) -> Option<(Module, Census)> {
    if facts.record_locals.is_empty() {
        return None;
    }
    let shapes = shapes(hir, facts);
    let cands: HashMap<LocalId, FactsRecRep> = facts
        .record_locals
        .iter()
        .filter(|(_, r)| shapes.contains_key(&r.tmpl))
        .map(|(id, r)| (*id, *r))
        .collect();
    if cands.is_empty() {
        return None;
    }
    let census = Refs::of(hir, &cands);
    let mut eligible: HashSet<LocalId> = cands
        .keys()
        .copied()
        .filter(|id| !census.bad.contains(id) && census.lets.get(id) == Some(&1))
        .collect();
    let next = perry_transform::generator::compute_max_local_id(hir).saturating_add(1);
    // A binding whose references the rewrite did not all reach stays a
    // heap object; the others are rewritten again without it.
    while !eligible.is_empty() {
        let mut out = hir.clone();
        let mut rw = Rewriter {
            shapes: &shapes,
            cands: &cands,
            eligible: &eligible,
            recs: HashMap::new(),
            next,
            reached: HashMap::new(),
            failed: HashSet::new(),
            census: Census::default(),
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
            trace(hir, &c);
            return Some((out, c));
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

fn trace(hir: &Module, c: &Census) {
    if std::env::var("PERRY_VALUE_RECORDS_TRACE").as_deref() == Ok("1") {
        eprintln!(
            "perry: value records: module {}: locals {} kept {} field reads {} presence {} materialized {} unpacked {}",
            hir.name, c.locals, c.kept, c.field_reads, c.presence, c.materialized, c.unpacked
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
    fn of(hir: &Module, cands: &HashMap<LocalId, FactsRecRep>) -> Refs {
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
                        r.bad.extend(ids.iter().filter(|id| cands.contains_key(*id)));
                    }
                    _ => {}
                }
            }
        });
        for f in all_functions(hir) {
            for p in &f.params {
                if cands.contains_key(&p.id) {
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
    /// The bindings declared so far, in traversal order.
    recs: HashMap<LocalId, Rec>,
    next: LocalId,
    /// References rewritten, per binding.
    reached: HashMap<LocalId, usize>,
    /// Bindings a use could not be rewritten for.
    failed: HashSet<LocalId>,
    census: Census,
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
        self.stmts(&mut f.body);
    }

    fn stmts(&mut self, list: &mut Vec<Stmt>) {
        let old = std::mem::take(list);
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
                        self.declare(id, &name, mutable, init, list);
                        continue;
                    }
                }
            }
            self.stmt(&mut s);
            list.push(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::Return(Some(e)) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
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
                    self.expr(u);
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
                Some(props.iter().cloned().enumerate().map(|(i, (_, v))| (i, v)).collect())
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
        let rec = Rec {
            tmpl: r.tmpl,
            track: r.opt || init.is_none(),
            present: self.fresh(),
            fields: keys.iter().map(|_| self.fresh()).collect(),
            tmp: self.fresh(),
            next: keys.iter().map(|_| self.fresh()).collect(),
        };
        // The scratch locals an assignment uses (a `const` has none).
        let scratch = |rec: &Rec, out: &mut Vec<Stmt>| {
            out.push(let_stmt(
                rec.tmp,
                format!("{name}$boxed"),
                Type::Any,
                true,
                undefined(),
            ));
            for (i, k) in keys.iter().enumerate() {
                out.push(let_stmt(
                    rec.next[i],
                    format!("{name}${k}$next"),
                    Type::Any,
                    true,
                    undefined(),
                ));
            }
        };
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
                        Type::Boolean,
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
                    scratch(&rec, out);
                }
                self.recs.insert(id, rec);
            }
            None => {
                if rec.track {
                    out.push(let_stmt(
                        rec.present,
                        format!("{name}$present"),
                        Type::Boolean,
                        true,
                        Expr::Bool(false),
                    ));
                }
                for (i, k) in keys.iter().enumerate() {
                    out.push(let_stmt(
                        rec.fields[i],
                        field_name(k),
                        Type::Any,
                        true,
                        undefined(),
                    ));
                }
                scratch(&rec, out);
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
                    else_expr: Box::new(undefined()),
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
            let i = shape.keys.iter().position(|x| x == k).expect("a layout key");
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
            Birth::Object => Expr::Object(shape.keys.iter().map(|k| (k.clone(), field(k))).collect()),
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
    fn cond(&mut self, e: &mut Expr) {
        if let Some(u) = self.rec_of(e) {
            self.reach(u);
            self.census.presence += 1;
            *e = self.presence_of(&self.recs[&u].clone());
            return;
        }
        self.expr(e);
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Some(u) = self.rec_of(e) {
            self.reach(u);
            *e = self.materialize(u);
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
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                byte_end,
            } if self.rec_of(object).is_some() => {
                let u = self.rec_of(object).unwrap();
                let rec = self.recs[&u].clone();
                match self.shapes[&rec.tmpl].keys.iter().position(|k| k == property) {
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
                self.stmts(body);
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
