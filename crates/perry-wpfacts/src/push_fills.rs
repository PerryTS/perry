//! Push-filled births: an Array born empty by an instance field initializer
//! (`k = []`, `k = new Array()`) and filled by `this.k.push(v)` in its
//! class's constructor before anything else can see it. The second
//! flow-sensitive walk beside `ctor_phase` (the solver is flow-insensitive,
//! so "every append precedes the first other use" cannot come from it).
//!
//! **The fill phase** (`analyze`), in program order from the initializer on
//! (the later field initializers, the parameter defaults, the top-level
//! statements of the constructor body) of a class without a parent:
//! - **construction phase.** `this` is used only as a read `this.p` (not as
//!   a callee) or a store `this.p = v` of a field already defined. Any other
//!   use of `this` (bare, a method call on it, a closure that mentions it, a
//!   key that is no defined field, `super`) closes the phase. While it is
//!   open no code but the constructor's own text can reach the instance, and
//!   the Array has no other name than `this.k`, so nothing else can reach
//!   the Array;
//! - **a fill statement** is `this.k.push(v);` as a top-level statement of
//!   the constructor body (one append), or as a top-level statement of the
//!   body of a top-level **counted loop** `for (let i = A; i < B; i++)`
//!   (`<=` too): `A` and `B` are integer constants (literals and `+ - *` of
//!   them), nothing else writes `i`, no closure captures it, and the body
//!   holds no `break`, `continue` or `return` (`B - A` appends). `v` neither
//!   mentions a field that is being filled nor closes the phase. A throw
//!   ends the constructor, and the instance is never seen;
//! - **the seal.** The first other mention of `this.k`, a `return`, the
//!   phase closing or the end of the body seals the field: its final length
//!   `N` is the appends counted so far. A `push` after the seal is no fill
//!   statement: the solver takes it as the ordinary append it is.
//!
//! So at every point any code but the fill can observe the Array, its
//! length is exactly `N` and no slot below `N` is a hole, and the solver may
//! take the fill as one birth in `N` steps:
//! - the template's birth length is `N` (`Model::arr_birth_len`);
//! - a fill push is an element store that is no append for the Arrays of its
//!   own fill (`Model::arr_len_store`; for any other Array that reaches the
//!   receiver it is the append it would be anyway) and no generic use
//!   (`rest_elems::use_of`).
//!
//! Such a template is a candidate for I32 at rest (`rest_elems::admit`)
//! beside the fused fills. **Export** (`rest_elems::export`): the push sites
//! of the at-rest fills by their receiver's `PropertyGet::byte_offset`, with
//! `N` (`ModuleBindings::rest_pushes`). Codegen lowers such a push as one
//! `js_array_rest_push`, whose first append gives the Array its `N` slots
//! and the I32 layout (the literal has no key codegen and the facts share).
//!
//! `PERRY_REST_PUSH_FILL=0` (also `off`, `false`) recognizes no fill: main's
//! facts. Negative control `PERRY_FACTS_SABOTAGE=restpush`: a push under an
//! `if` in a counted loop counts as unconditional.

use crate::model::{Model, NodeId, TmplId};
use crate::pts::FxHashSet;
use perry_hir::walker::walk_expr_children;
use perry_hir::{BinaryOp, Class, CompareOp, Expr, Stmt, UpdateOp};
use std::collections::{BTreeMap, BTreeSet};

/// `PERRY_REST_PUSH_FILL=0` turns the fill phase off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REST_PUSH_FILL").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// One push-filled field of a class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fill {
    /// The field `k`.
    pub field: String,
    /// The final length `N`.
    pub len: u32,
    /// Identity (`*const Expr as usize`) of the field's initializer.
    pub birth: usize,
    /// Each fill push: the identity of its `push` expression and the byte
    /// offset of its receiver `this.k`.
    pub pushes: Vec<(usize, u32)>,
}

/// A fill push the generator walked (`Gen::expr_rest`): the module, the
/// receiver's offset (codegen's key), the receiver node, the fill's index in
/// `Model::push_fills`.
#[derive(Clone, Copy, Debug)]
pub struct FillPush {
    pub module: u32,
    pub offset: u32,
    pub recv: NodeId,
    pub fill: u32,
}

/// `this.k.push(v)` as HIR folds it for a field receiver: `(k, the
/// receiver's offset, v)`.
fn this_push(e: &Expr) -> Option<(&str, u32, &Expr)> {
    let Expr::NativeMethodCall {
        module,
        method,
        object: Some(object),
        args,
        ..
    } = e
    else {
        return None;
    };
    if module != "array" || method != "push_single" || args.len() != 1 {
        return None;
    }
    let Expr::PropertyGet {
        object: owner,
        property,
        byte_offset,
        ..
    } = object.as_ref()
    else {
        return None;
    };
    (matches!(owner.as_ref(), Expr::This) && *byte_offset != 0)
        .then(|| (property.as_str(), *byte_offset, &args[0]))
}

/// An empty Array birth: `[]`, or `new Array()`.
fn empty_birth(e: &Expr) -> bool {
    match e {
        Expr::Array(elems, _) => elems.is_empty(),
        Expr::New {
            class_name, args, ..
        } => class_name == "Array" && args.is_empty(),
        _ => false,
    }
}

/// The value of an integer constant expression.
fn int_const(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(i) => Some(*i),
        Expr::Number(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
        Expr::Binary { op, left, right } => {
            let (a, b) = (int_const(left)?, int_const(right)?);
            match op {
                BinaryOp::Add => a.checked_add(b),
                BinaryOp::Sub => a.checked_sub(b),
                BinaryOp::Mul => a.checked_mul(b),
                _ => None,
            }
            .filter(|v| v.abs() < 9_000_000_000_000_000)
        }
        _ => None,
    }
}

/// What a piece of constructor text does with `this`.
#[derive(Default)]
struct Uses {
    /// The phase closes (module docs).
    leak: bool,
    /// The defined fields it reads or stores as `this.p`.
    mentions: BTreeSet<String>,
}

struct Scan<'c> {
    /// The fields defined so far.
    defined: &'c BTreeSet<&'c str>,
    uses: Uses,
}

impl Scan<'_> {
    fn this_key(&mut self, p: &str) {
        if self.defined.contains(p) {
            self.uses.mentions.insert(p.to_string());
        } else {
            // Not an own data property yet: the lookup walks the prototype
            // chain, where an accessor runs with `this`.
            self.uses.leak = true;
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::This
            | Expr::ThisProvenClass(_)
            | Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. } => {
                self.uses.leak = true;
                walk_expr_children(e, &mut |c| self.expr(c));
            }
            // `this.f(...)`: the callee runs with `this` as its receiver.
            Expr::Call { callee, .. }
            | Expr::CallSpread { callee, .. }
            | Expr::JsCallValue { callee, .. }
                if matches!(callee.as_ref(), Expr::PropertyGet { object, .. }
                    | Expr::IndexGet { object, .. }
                    if matches!(object.as_ref(), Expr::This)) =>
            {
                self.uses.leak = true;
                walk_expr_children(e, &mut |c| self.expr(c));
            }
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => self.this_key(property),
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } if matches!(object.as_ref(), Expr::This) => {
                self.expr(value);
                self.this_key(property);
            }
            Expr::Closure {
                captures_this,
                body,
                ..
            } => {
                if *captures_this || crate::ctor_phase::body_mentions_this(body) {
                    self.uses.leak = true;
                }
                walk_expr_children(e, &mut |c| self.expr(c));
            }
            _ => walk_expr_children(e, &mut |c| self.expr(c)),
        }
    }
}

fn uses_of_expr(defined: &BTreeSet<&str>, e: &Expr) -> Uses {
    let mut s = Scan {
        defined,
        uses: Uses::default(),
    };
    s.expr(e);
    s.uses
}

fn uses_of_stmt(defined: &BTreeSet<&str>, st: &Stmt) -> Uses {
    let mut s = Scan {
        defined,
        uses: Uses::default(),
    };
    perry_hir::walker::stmt_any_expr(st, &mut |e| {
        s.expr(e);
        false
    });
    s.uses
}

/// A statement of `ss` (nested ones included, closures not entered)
/// satisfies `f`.
fn any_stmt(ss: &[Stmt], f: &mut dyn FnMut(&Stmt) -> bool) -> bool {
    ss.iter().any(|s| {
        if f(s) {
            return true;
        }
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => any_stmt(then_branch, f) || else_branch.as_ref().is_some_and(|b| any_stmt(b, f)),
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => any_stmt(body, f),
            Stmt::For { init, body, .. } => {
                init.as_ref()
                    .is_some_and(|i| any_stmt(std::slice::from_ref(i.as_ref()), f))
                    || any_stmt(body, f)
            }
            Stmt::Labeled { body, .. } => any_stmt(std::slice::from_ref(body.as_ref()), f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                any_stmt(body, f)
                    || catch.as_ref().is_some_and(|c| any_stmt(&c.body, f))
                    || finally.as_ref().is_some_and(|b| any_stmt(b, f))
            }
            Stmt::Switch { cases, .. } => cases.iter().any(|c| any_stmt(&c.body, f)),
            Stmt::Let { .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Throw(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => false,
        }
    })
}

fn returns(ss: &[Stmt]) -> bool {
    any_stmt(ss, &mut |s| matches!(s, Stmt::Return(_)))
}

fn jumps(ss: &[Stmt]) -> bool {
    any_stmt(ss, &mut |s| {
        matches!(
            s,
            Stmt::Return(_)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
        )
    })
}

/// An expression of `ss` (closures not entered, their capture lists read)
/// may write local `id`.
fn writes_local(ss: &[Stmt], id: u32) -> bool {
    fn ex(e: &Expr, id: u32) -> bool {
        match e {
            Expr::LocalSet(l, _) if *l == id => return true,
            Expr::Update { id: l, .. } if *l == id => return true,
            Expr::Closure {
                captures,
                mutable_captures,
                ..
            } if captures.contains(&id) || mutable_captures.contains(&id) => return true,
            _ => {}
        }
        let mut found = false;
        walk_expr_children(e, &mut |c| found = found || ex(c, id));
        found
    }
    ss.iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| ex(e, id)))
        // A redeclaration is another local; a `let` of the same id is none.
        || any_stmt(ss, &mut |s| matches!(s, Stmt::Let { id: l, .. } if *l == id))
}

/// The trip count of a counted loop (module docs), with its body.
fn counted(s: &Stmt) -> Option<(i64, &[Stmt])> {
    let Stmt::For {
        init: Some(init),
        condition: Some(condition),
        update: Some(update),
        body,
    } = s
    else {
        return None;
    };
    let Stmt::Let {
        id,
        init: Some(from),
        ..
    } = init.as_ref()
    else {
        return None;
    };
    let from = int_const(from)?;
    let Expr::Compare { op, left, right } = condition else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(l) if l == id) {
        return None;
    }
    let to = int_const(right)?;
    let to = match op {
        CompareOp::Lt => to,
        CompareOp::Le => to.checked_add(1)?,
        _ => return None,
    };
    if !matches!(update, Expr::Update { id: l, op: UpdateOp::Increment, .. } if l == id) {
        return None;
    }
    if jumps(body) || writes_local(body, *id) {
        return None;
    }
    Some((to.checked_sub(from)?.max(0), body))
}

struct Cand {
    field: String,
    birth: usize,
    count: i64,
    pushes: Vec<(usize, u32)>,
    sealed: bool,
}

struct Walk {
    cands: Vec<Cand>,
    sabotage: bool,
}

impl Walk {
    fn seal(&mut self, names: &BTreeSet<String>) {
        for c in &mut self.cands {
            if names.contains(&c.field) {
                c.sealed = true;
            }
        }
    }

    fn seal_all(&mut self) {
        for c in &mut self.cands {
            c.sealed = true;
        }
    }

    /// Text that is no fill statement: what it mentions is sealed; `false`
    /// when it closes the phase.
    fn other(&mut self, uses: Uses) -> bool {
        self.seal(&uses.mentions);
        !uses.leak
    }

    /// `s` as the fill push `this.k.push(v);` of an open candidate, run
    /// `times` times: counted. `Some(false)`: its value closes the phase.
    fn push(&mut self, defined: &BTreeSet<&str>, s: &Stmt, times: i64) -> Option<bool> {
        let Stmt::Expr(e) = s else {
            return None;
        };
        let (k, offset, v) = this_push(e)?;
        let at = self.cands.iter().position(|c| c.field == k && !c.sealed)?;
        let uses = uses_of_expr(defined, v);
        if uses.leak {
            return Some(false);
        }
        // The value is evaluated before the append: a field it mentions is
        // observed there.
        self.seal(&uses.mentions);
        let c = &mut self.cands[at];
        if c.sealed {
            return None;
        }
        c.count = c.count.saturating_add(times);
        c.pushes.push((e as *const Expr as usize, offset));
        Some(true)
    }

    /// The fields a statement of a counted loop's body observes: a fill push
    /// observes what its value mentions, any other statement what it
    /// mentions.
    fn observed(&self, defined: &BTreeSet<&str>, s: &Stmt, seen: &mut BTreeSet<String>) {
        if let Stmt::Expr(e) = s {
            if let Some((k, _, v)) = this_push(e) {
                if self.cands.iter().any(|c| c.field == k && !c.sealed) {
                    seen.extend(uses_of_expr(defined, v).mentions);
                    return;
                }
            }
        }
        if let (true, Stmt::If { then_branch, .. }) = (self.sabotage, s) {
            // The negative control: a conditional append is a fill push.
            for t in then_branch {
                self.observed(defined, t, seen);
            }
            return;
        }
        seen.extend(uses_of_stmt(defined, s).mentions);
    }

    /// One top-level statement of the constructor body; `false` when the
    /// phase closes in it.
    fn stmt(&mut self, defined: &BTreeSet<&str>, s: &Stmt) -> bool {
        if let Some(open) = self.push(defined, s, 1) {
            return open;
        }
        if let Some((trips, body)) = counted(s) {
            // A loop that closes the phase anywhere is no fill: a later
            // iteration's append runs after the leak.
            if !uses_of_stmt(defined, s).leak {
                // What the body observes, it observes in its first
                // iteration, before the appends of the later ones: sealed
                // before any append of this loop counts.
                let mut seen = BTreeSet::new();
                for b in body {
                    self.observed(defined, b, &mut seen);
                }
                self.seal(&seen);
                for b in body {
                    if self.push(defined, b, trips).is_some() {
                        continue;
                    }
                    if let (true, Stmt::If { then_branch, .. }) = (self.sabotage, b) {
                        for t in then_branch {
                            let _ = self.push(defined, t, trips);
                        }
                    }
                }
                return true;
            }
        }
        let open = self.other(uses_of_stmt(defined, s));
        if returns(std::slice::from_ref(s)) {
            // An instance may leave the constructor here, short of the
            // appends after it.
            self.seal_all();
        }
        open
    }
}

/// The push-filled fields of class `c` (module docs). `sabotage`: the
/// negative control `restpush`.
pub fn analyze(c: &Class, sabotage: bool) -> Vec<Fill> {
    if c.extends.is_some()
        || c.extends_name.is_some()
        || c.native_extends.is_some()
        || c.extends_expr.is_some()
    {
        return Vec::new();
    }
    let Some(ctor) = &c.constructor else {
        return Vec::new();
    };
    let named = |f: &perry_hir::ClassField| {
        !f.is_private && f.key_expr.is_none() && f.origin.is_definition()
    };
    // A name two fields, a method or an accessor share is no candidate.
    let unique = |name: &str| {
        c.fields.iter().filter(|f| f.name == name).count() == 1
            && !c.methods.iter().any(|m| m.name == name)
            && !c.getters.iter().any(|(n, _)| n == name)
            && !c.setters.iter().any(|(n, _)| n == name)
    };
    let mut w = Walk {
        cands: Vec::new(),
        sabotage,
    };
    let mut defined: BTreeSet<&str> = BTreeSet::new();
    let mut open = true;
    for f in &c.fields {
        if let Some(k) = &f.key_expr {
            open = open && w.other(uses_of_expr(&defined, k));
        }
        if let Some(init) = &f.init {
            open = open && w.other(uses_of_expr(&defined, init));
        }
        if !open {
            break;
        }
        if named(f) {
            defined.insert(f.name.as_str());
            if f.init.as_ref().is_some_and(empty_birth) && unique(&f.name) {
                w.cands.push(Cand {
                    field: f.name.clone(),
                    birth: f.init.as_ref().map_or(0, |e| e as *const Expr as usize),
                    count: 0,
                    pushes: Vec::new(),
                    sealed: false,
                });
            }
        }
    }
    if open {
        for p in &ctor.params {
            if let Some(d) = &p.default {
                open = open && w.other(uses_of_expr(&defined, d));
            }
        }
    }
    if open {
        for s in &ctor.body {
            if !w.stmt(&defined, s) {
                break;
            }
            if w.cands.iter().all(|c| c.sealed) {
                break;
            }
        }
    }
    let dense = i64::from(perry_abi::MAX_FRESH_DENSE_ARRAY_LENGTH);
    w.cands
        .into_iter()
        .filter(|c| (1..=dense).contains(&c.count) && !c.pushes.is_empty())
        .map(|c| Fill {
            field: c.field,
            len: c.count as u32,
            birth: c.birth,
            pushes: c.pushes,
        })
        .collect()
}

/// The fills of a program, as the generator and the solver hold them.
#[derive(Default, Clone, Debug)]
pub struct Fills {
    /// Each fill's final length, by its index.
    pub lens: Vec<u32>,
    /// A birth expression's fill, by identity.
    pub births: BTreeMap<usize, u32>,
    /// A fill push expression's fill, by identity.
    pub pushes: BTreeMap<usize, u32>,
}

impl Fills {
    /// Take the fills of one class.
    pub(crate) fn add(&mut self, fills: Vec<Fill>) {
        for f in fills {
            let id = self.lens.len() as u32;
            self.lens.push(f.len);
            self.births.insert(f.birth, id);
            for (e, _) in f.pushes {
                self.pushes.insert(e, id);
            }
        }
    }
}

/// Template `tm` is born by fill `fill`: its birth length is the fill's
/// final length.
pub(crate) fn born(m: &mut Model, tm: TmplId, fill: u32) {
    let Some(&len) = m.push_fill_lens.get(fill as usize) else {
        return;
    };
    m.push_fill_tmpls.insert(tm, fill);
    m.arr_birth_len.insert(tm, i64::from(len));
}

/// The literal `e`, whose template is `tm`, is a fill's birth: [`born`].
pub(crate) fn born_at(g: &mut crate::gen::Gen<'_>, e: &Expr, tm: TmplId) -> bool {
    let fill = g
        .push_fill
        .births
        .get(&(e as *const Expr as usize))
        .copied();
    if let Some(fill) = fill {
        born(&mut g.m, tm, fill);
    }
    fill.is_some()
}

/// The fill whose Arrays node `n` holds, when it holds nothing else and
/// every one is of a template in `cand`.
pub(crate) fn only_fill(m: &Model, n: NodeId, cand: &FxHashSet<TmplId>, fill: u32) -> bool {
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return false;
    }
    node.pts.to_sorted().into_iter().all(|t| {
        t != crate::model::TOP
            && m.tmpl_of(t)
                .is_some_and(|tm| cand.contains(&tm) && m.push_fill_tmpls.get(&tm) == Some(&fill))
    })
}
