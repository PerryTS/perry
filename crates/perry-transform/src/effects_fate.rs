//! The fate of each parameter (and `this`) of a function: what running the
//! body does with the value that arrives there.
//!
//! The lattice, bottom to top: `Dropped < Returned < KeptByReceiver < Kept`.
//!
//! * `Dropped`: the value does not outlive the call through this call.
//! * `Returned`: it leaves only as the call's result.
//! * `KeptByReceiver`: it is stored in a field of the receiver.
//! * `Kept`: stored elsewhere, or used in a way the rule does not read. This
//!   is the default for everything the rule does not recognise.
//!
//! A body drops a parameter `q` when every occurrence of `q` is one of:
//!
//! * the receiver of a field read the proof says runs no code
//!   ([`FateProof::read`]): an own data slot, never a getter, proxy or an
//!   inherited or absent key. The shape proves it, never a declared type;
//! * an operand of `===`/`!==`, or of `==`/`!=` against `null`/`undefined`
//!   (an address compare: no code runs);
//! * the operand of `typeof`, or a value only tested for truthiness or
//!   discarded: an `if`/`while`/`for` condition, `!q`, `Boolean(q)`, the
//!   condition of `?:`, an operand of `&&`/`||`/`??` whose result is itself
//!   only tested, an expression statement. Neither `typeof` nor ToBoolean
//!   runs code, on any value (a proxy has no trap for either);
//! * an operand of an operator that converts it (ToPrimitive, ToNumeric,
//!   ToString: arithmetic, bitwise, unary `-`/`+`/`~`, `<`/`<=`/`>`/`>=`,
//!   `==`/`!=` against anything but `null`/`undefined`, a template
//!   substitution, `String(q)`, `Number(q)`) when the proof says the
//!   binding holds only Numbers ([`FateProof::primitive`]). On an object
//!   these call `valueOf`/`toString`/`Symbol.toPrimitive` with `q` as
//!   `this`, which keeps it;
//! * argument `i` of a call all of whose bodies ([`FateProof::targets`])
//!   drop their parameter `i`, or of `new C(...)` whose constructor drops it.
//!
//! Assigning the binding (`q = v`) keeps nothing of the value it held: `v`
//! flows into `q`'s binding and is read wherever `q` is read (a tracked
//! binding assigned into another one keeps the assigned one). A copy
//! `let r = q` whose `r` is never written in the function (the writes-only
//! occurrence census `scoped_copies` folds copies by) is `q` under another
//! name: every read of `r`, a capture included, is a read of `q`.
//!
//! `return q` makes it `Returned`; a store `this.k = q` the proof accepts
//! ([`FateProof::store`]) makes it `KeptByReceiver`. In the constructor of
//! a closed object literal's shape class ([`Class::is_literal_shape`]) every
//! store is one: that check admits only positional `this.<field> = <param>`
//! stores of a class without accessors, methods or a parent, the literal
//! defines its own keys (no setter up the chain runs), and codegen lowers
//! them as direct slot stores into the object being born. Everything else keeps
//! it: assignment to any local, field, element or global, capture, `throw`,
//! spread, any operator, a method call on it, a call whose bodies are not all
//! known, a runtime or built-in call. A function whose body the rule does not
//! walk (async, generator, `arguments`, captures, decorators) keeps every
//! parameter.
//!
//! A default value is evaluated in the callee, before the body, and may read
//! earlier parameters: its expression is walked as a use like any other. A
//! rest parameter is a fresh array holding the arguments from its position
//! on: the rule keeps those arguments (the array's elements are read out as
//! values the rule does not follow), and walks the body for the others.
//!
//! Across calls the fates are a least fixpoint over the call graph: strongly
//! connected components bottom-up, and inside one component every function
//! starts at `Dropped` and is re-read until nothing moves. Recursion that only
//! passes `q` on therefore stays `Dropped`, and one keep anywhere in a cycle
//! keeps it in the whole cycle.
//!
//! A fate rests on proofs. It is `Proven` when every read and every call
//! target set it rests on is proven without a run-time guard, and `Bet(site)`
//! when one of them is guarded; `site` names the first such site.

use std::collections::HashMap;

use perry_hir::types::{FuncId, LocalId};
use perry_hir::{Class, CompareOp, Expr, Function, Stmt, UnaryOp};

/// See the module comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fate {
    Dropped,
    Returned,
    KeptByReceiver,
    Kept,
}

/// What a fate below `Kept` rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proof {
    /// Proven without a run-time guard.
    Proven,
    /// Rests on a guarded site (its id, from the proof that answered).
    Bet(u64),
}

impl Proof {
    fn join(self, other: Proof) -> Proof {
        match self {
            Proof::Proven => other,
            bet => bet,
        }
    }
}

/// A short name carried in a [`Why`]: an HIR node's variant name, a
/// property or a class name, cut at 31 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Name {
    len: u8,
    bytes: [u8; 31],
}

impl Name {
    pub fn new(s: &str) -> Name {
        let mut n = Name {
            len: 0,
            bytes: [0; 31],
        };
        n.push(s);
        n
    }

    fn push(&mut self, s: &str) {
        for c in s.chars() {
            let at = self.len as usize;
            if at + c.len_utf8() > self.bytes.len() {
                return;
            }
            c.encode_utf8(&mut self.bytes[at..]);
            self.len += c.len_utf8() as u8;
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("?")
    }

    /// The variant name of `e` (`LocalGet`, `Object`, `JsonStringify`...):
    /// the head of its derived `Debug`, which stops at the first byte that
    /// is not part of an identifier.
    pub fn of_expr(e: &Expr) -> Name {
        struct Head(Name, bool);
        impl std::fmt::Write for Head {
            fn write_str(&mut self, s: &str) -> std::fmt::Result {
                if self.1 {
                    return Err(std::fmt::Error);
                }
                for (i, c) in s.char_indices() {
                    if !(c.is_alphanumeric() || c == '_') {
                        self.0.push(&s[..i]);
                        self.1 = true;
                        return Err(std::fmt::Error);
                    }
                }
                self.0.push(s);
                Ok(())
            }
        }
        let mut h = Head(Name::new(""), false);
        let _ = std::fmt::write(&mut h, format_args!("{e:?}"));
        h.0
    }
}

impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a binding is `Kept`: its first keeping use in the walk that raised
/// it. Data beside the fate; no fate reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Why {
    /// The body is not walked (`ordinary` is false): which part.
    NotOrdinary(&'static str),
    /// Read under a node the rule does not read: that node's name (an HIR
    /// `Expr` variant, or the statement: `Let`, `If`, `Expr`, `Throw`...).
    Use(Name),
    /// `q.k` the proof does not answer.
    Read(Name),
    /// `this.k = v` the proof does not admit (keeps `this`).
    Store(Name),
    /// `q.m(..)` whose bodies are not known: the property.
    Receiver(Name),
    /// `q.m(..)` whose known bodies keep `this`.
    ReceiverKept,
    /// An argument of a call whose bodies are not known: the callee's node
    /// (`PropertyGet.<k>`, `LocalGet`, ...).
    ArgUnresolved(Name),
    /// An argument at a position a known body does not drop: that fate.
    ArgKnown(Fate),
    /// An argument of a call with a known body that has no fate (another
    /// module's, a closure that captures).
    ArgNoFate,
    /// An argument of `new C` for a class that is not one plain class here.
    NewUnknown(Name),
    /// Referenced by a closure.
    Captured,
    /// `this` captured by an arrow.
    CapturesThis,
    /// Assigned (`q = ...`).
    Reassigned,
    /// `q++` / `q--`.
    Updated,
    /// A mutation that names the binding by id: the node.
    Mutated(&'static str),
    /// An argument collected by a rest parameter.
    Rest,
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Why::NotOrdinary(k) => write!(f, "not-ordinary({k})"),
            Why::Use(n) => write!(f, "use({})", n.as_str()),
            Why::Read(n) => write!(f, "read({})", n.as_str()),
            Why::Store(n) => write!(f, "store({})", n.as_str()),
            Why::Receiver(n) => write!(f, "receiver({})", n.as_str()),
            Why::ReceiverKept => f.write_str("receiver-kept"),
            Why::ArgUnresolved(n) => write!(f, "arg-unresolved({})", n.as_str()),
            Why::ArgKnown(k) => write!(f, "arg-to({k:?})"),
            Why::ArgNoFate => f.write_str("arg-no-fate"),
            Why::NewUnknown(n) => write!(f, "new-unknown({})", n.as_str()),
            Why::Captured => f.write_str("captured"),
            Why::CapturesThis => f.write_str("captures-this"),
            Why::Reassigned => f.write_str("reassigned"),
            Why::Updated => f.write_str("updated"),
            Why::Mutated(k) => write!(f, "mutated({k})"),
            Why::Rest => f.write_str("rest"),
        }
    }
}

/// The fate of one parameter, with what it rests on (`Proven` for `Kept`),
/// and for `Kept` the first keeping use (`None` when no body was read).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamFate {
    pub fate: Fate,
    pub proof: Proof,
    pub why: Option<Why>,
}

impl ParamFate {
    pub const DROPPED: ParamFate = ParamFate {
        fate: Fate::Dropped,
        proof: Proof::Proven,
        why: None,
    };
    pub const KEPT: ParamFate = ParamFate {
        fate: Fate::Kept,
        proof: Proof::Proven,
        why: None,
    };

    fn raise(&mut self, fate: Fate, proof: Proof, why: Option<Why>) {
        self.fate = self.fate.max(fate);
        self.proof = if self.fate == Fate::Kept {
            Proof::Proven
        } else {
            self.proof.join(proof)
        };
        if self.fate == Fate::Kept && self.why.is_none() {
            self.why = why;
        }
    }

    /// Dropped on proofs alone, no bet.
    pub fn proven_dropped(&self) -> bool {
        self.fate == Fate::Dropped && self.proof == Proof::Proven
    }
}

/// The fates of one function: one per declared parameter, and its `this`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnFates {
    pub params: Vec<ParamFate>,
    pub this: ParamFate,
    /// The fate of an argument past the declared parameters.
    tail: ParamFate,
}

impl FnFates {
    fn all(f: &Function, fate: ParamFate) -> Self {
        Self {
            params: vec![fate; f.params.len()],
            this: fate,
            tail: tail(f),
        }
    }

    /// The fate of argument position `i`. An argument past the declared
    /// parameters reaches a binding only through a rest parameter or
    /// `arguments` (both keep it).
    pub fn arg(&self, i: usize) -> ParamFate {
        self.params.get(i).copied().unwrap_or(self.tail)
    }
}

/// The fate of an argument past `f`'s declared parameters: kept when a rest
/// parameter or `arguments` collects it, otherwise no binding sees it.
fn tail(f: &Function) -> ParamFate {
    if let Some(k) = f.params.iter().find_map(|p| {
        if p.arguments_object.is_some() {
            Some(Why::NotOrdinary("arguments"))
        } else if p.is_rest {
            Some(Why::Rest)
        } else {
            None
        }
    }) {
        ParamFate {
            why: Some(k),
            ..ParamFate::KEPT
        }
    } else {
        ParamFate::DROPPED
    }
}

/// The two questions the rule cannot answer from the body alone. Each
/// consumer answers them from its own facts, and every consumer shares this
/// one rule.
pub trait FateProof {
    /// `object.property` in the body of `func`, where `object` is a
    /// parameter's `LocalGet` or `This`: does the read only load an own data
    /// slot, running no code? `None` when not proven. `site` is the read's
    /// byte offset, the id a guarded answer names in `Proof::Bet`.
    fn read(&self, func: FuncId, object: &Expr, property: &str, site: u32) -> Option<Proof>;
    /// `this.property = value` in the body of `func`: does the store only
    /// write an own data slot of the receiver, running no code?
    fn store(&self, func: FuncId, property: &str) -> Option<Proof>;
    /// Every body a call to `callee` can run, or `None` when not all known.
    fn targets(&self, callee: &Expr) -> Option<(Vec<FuncId>, Proof)>;
    /// Does the binding `local` of `func` hold only primitives that are no
    /// heap object (Numbers), so no conversion of it runs code? `None` when
    /// not proven.
    fn primitive(&self, _func: FuncId, _local: LocalId) -> Option<Proof> {
        None
    }
}

/// The answers known without whole-program facts: no read or store is
/// proven, and a call's bodies are known only for a direct function
/// reference.
pub struct BodyOnly;

impl FateProof for BodyOnly {
    fn read(&self, _: FuncId, _: &Expr, _: &str, _: u32) -> Option<Proof> {
        None
    }
    fn store(&self, _: FuncId, _: &str) -> Option<Proof> {
        None
    }
    fn targets(&self, callee: &Expr) -> Option<(Vec<FuncId>, Proof)> {
        direct_target(callee)
    }
}

/// `Call { callee: FuncRef }` names exactly one body.
pub fn direct_target(callee: &Expr) -> Option<(Vec<FuncId>, Proof)> {
    match callee {
        Expr::FuncRef(id) => Some((vec![*id], Proof::Proven)),
        _ => None,
    }
}

/// The fates of every ordinary function, method and constructor of a
/// module. A function absent from the table keeps every parameter.
#[derive(Default, Debug)]
pub struct Fates {
    by_fn: HashMap<FuncId, FnFates>,
    /// `new <class>`: the constructor's id when the name is one class that
    /// extends nothing (so no other constructor body runs). `None` for a
    /// class without a constructor: construction drops every argument.
    construction: HashMap<String, Option<FuncId>>,
}

impl Fates {
    pub fn of_module(module: &perry_hir::Module, proof: &dyn FateProof) -> Self {
        let classes: Vec<&Class> = module.classes.iter().collect();
        let functions: Vec<&Function> = module.functions.iter().collect();
        Self::compute(&functions, &classes, proof)
    }

    /// The fates of `functions` and of the classes' constructors, methods
    /// and static methods.
    pub fn compute(functions: &[&Function], classes: &[&Class], proof: &dyn FateProof) -> Self {
        let mut count: HashMap<&str, usize> = HashMap::new();
        for class in classes {
            *count.entry(class.name.as_str()).or_insert(0) += 1;
        }
        let mut construction = HashMap::new();
        let mut bodies: Vec<&Function> = functions.to_vec();
        let literal: std::collections::HashSet<FuncId> = classes
            .iter()
            .filter(|c| c.is_literal_shape())
            .filter_map(|c| c.constructor.as_ref().map(|k| k.id))
            .collect();
        for class in classes {
            if let Some(ctor) = &class.constructor {
                bodies.push(ctor);
            }
            bodies.extend(class.methods.iter());
            bodies.extend(class.static_methods.iter());
            let plain = class.extends.is_none()
                && class.extends_name.is_none()
                && class.native_extends.is_none()
                && class.extends_expr.is_none()
                && class.decorators.is_empty();
            if plain && count[class.name.as_str()] == 1 {
                construction.insert(class.name.clone(), class.constructor.as_ref().map(|c| c.id));
            }
        }
        let mut fates = Fates {
            by_fn: HashMap::new(),
            construction,
        };
        let index: HashMap<FuncId, usize> =
            bodies.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
        // A second body with one id cannot be told apart from the first.
        if index.len() != bodies.len() {
            return fates;
        }
        let edges: Vec<Vec<usize>> = bodies
            .iter()
            .map(|f| {
                let mut out = Vec::new();
                callees(f, proof, &fates.construction, &mut |id| {
                    if let Some(&i) = index.get(&id) {
                        out.push(i);
                    }
                });
                out
            })
            .collect();
        for component in sccs(&edges) {
            for &i in &component {
                let f = bodies[i];
                let start = match not_walkable(f) {
                    None => ParamFate::DROPPED,
                    Some(k) => ParamFate {
                        why: Some(Why::NotOrdinary(k)),
                        ..ParamFate::KEPT
                    },
                };
                fates
                    .by_fn
                    .insert(f.id, FnFates::all(f, start));
            }
            loop {
                let mut moved = false;
                for &i in &component {
                    let f = bodies[i];
                    if not_walkable(f).is_some() {
                        continue;
                    }
                    let next = walk_body(f, proof, &fates, literal.contains(&f.id));
                    if fates.by_fn.get(&f.id) != Some(&next) {
                        fates.by_fn.insert(f.id, next);
                        moved = true;
                    }
                }
                if !moved {
                    break;
                }
            }
        }
        fates
    }

    pub fn of_fn(&self, id: FuncId) -> Option<&FnFates> {
        self.by_fn.get(&id)
    }

    /// The fate of argument `i` at a call with these bodies.
    pub fn call_arg(&self, targets: &[FuncId], i: usize) -> ParamFate {
        let mut out = ParamFate::DROPPED;
        for t in targets {
            match self.by_fn.get(t) {
                Some(f) => {
                    let a = f.arg(i);
                    out.raise(a.fate, a.proof, a.why);
                }
                None => return ParamFate::KEPT,
            }
        }
        out
    }

    /// The fate of argument `i` of `new <class>(...)`.
    pub fn construction_arg(&self, class: &str, i: usize) -> ParamFate {
        match self.construction.get(class) {
            Some(Some(ctor)) => self.call_arg(&[*ctor], i),
            Some(None) => ParamFate::DROPPED,
            None => ParamFate::KEPT,
        }
    }
}

/// `effects.rs`'s question (closed construction, the inliner): a body the
/// fate walks, without default or rest parameters.
pub(crate) fn ordinary(f: &Function) -> bool {
    not_walkable(f).is_none() && f.params.iter().all(|p| p.default.is_none() && !p.is_rest)
}

/// The first part that keeps the fate from walking `f`, `None` when it
/// walks it: a body that suspends, captures, is decorated, or sees its
/// arguments through `arguments` (in a sloppy function `arguments[i]` and
/// parameter `i` are one binding).
fn not_walkable(f: &Function) -> Option<&'static str> {
    if f.is_async {
        return Some("async");
    }
    if f.is_generator {
        return Some("generator");
    }
    if !f.captures.is_empty() {
        return Some("captures");
    }
    if !f.decorators.is_empty() {
        return Some("decorators");
    }
    for p in &f.params {
        if p.arguments_object.is_some() {
            return Some("arguments");
        }
        if !p.decorators.is_empty() {
            return Some("param-decorators");
        }
    }
    None
}

/// The fates `f`'s body gives its parameters, with callees read from `known`.
pub fn body_fates(f: &Function, proof: &dyn FateProof, known: &Fates) -> FnFates {
    walk_body(f, proof, known, false)
}

/// [`body_fates`]; `literal_ctor`: `f` constructs a literal shape, whose
/// `this` stores are own data stores by construction.
fn walk_body(f: &Function, proof: &dyn FateProof, known: &Fates, literal_ctor: bool) -> FnFates {
    let mut walk = Walk {
        literal_ctor,
        func: f.id,
        params: f.params.iter().map(|p| p.id).collect(),
        aliases: aliases(f),
        out: FnFates::all(f, ParamFate::DROPPED),
        proof,
        known,
        parent: Parent::Stmt("Expr"),
    };
    for (i, p) in f.params.iter().enumerate() {
        if p.is_rest {
            walk.keep(Slot::Param(i), |_| Why::Rest);
        }
    }
    // Defaults run in the callee before the body, in parameter order.
    for p in &f.params {
        if let Some(d) = &p.default {
            walk.at("Default");
            walk.expr(d);
        }
    }
    for stmt in &f.body {
        walk.stmt(stmt);
    }
    walk.out
}

/// Which tracked binding an expression is: a parameter, or `this`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Param(usize),
    This,
}

/// The node a binding read sits under: named only when that read keeps
/// the binding (`Why::Use`).
#[derive(Clone, Copy)]
enum Parent<'e> {
    Stmt(&'static str),
    Expr(&'e Expr),
}

struct Walk<'a, 'e> {
    /// The constructor of a literal shape ([`Class::is_literal_shape`]).
    literal_ctor: bool,
    func: FuncId,
    params: Vec<LocalId>,
    /// Never-written copies of a parameter: (copy, parameter index).
    aliases: Vec<(LocalId, usize)>,
    out: FnFates,
    proof: &'a dyn FateProof,
    known: &'a Fates,
    parent: Parent<'e>,
}

impl<'e> Walk<'_, 'e> {
    fn slot(&self, e: &Expr) -> Option<Slot> {
        match e {
            Expr::LocalGet(id) => self.param(*id),
            Expr::This => Some(Slot::This),
            _ => None,
        }
    }

    /// `this.property = v` writes an own data slot and runs no code: by
    /// construction in a literal shape's constructor, else the proof's.
    fn store(&self, property: &str) -> Option<Proof> {
        if self.literal_ctor {
            Some(Proof::Proven)
        } else {
            self.proof.store(self.func, property)
        }
    }

    fn param(&self, id: LocalId) -> Option<Slot> {
        self.params
            .iter()
            .position(|p| *p == id)
            .or_else(|| self.aliases.iter().find(|(q, _)| *q == id).map(|(_, i)| *i))
            .map(Slot::Param)
    }

    fn fate_mut(&mut self, slot: Slot) -> &mut ParamFate {
        match slot {
            Slot::Param(i) => &mut self.out.params[i],
            Slot::This => &mut self.out.this,
        }
    }

    /// Raises below `Kept`.
    fn raise(&mut self, slot: Slot, fate: Fate, proof: Proof) {
        debug_assert!(fate != Fate::Kept, "Kept goes through keep");
        self.fate_mut(slot).raise(fate, proof, None);
    }

    /// Raises to `Kept`; `why` names the use, computed only for the first.
    fn keep(&mut self, slot: Slot, why: impl FnOnce(&Self) -> Why) {
        if self.fate_mut(slot).why.is_some() {
            return;
        }
        let w = why(self);
        self.fate_mut(slot).raise(Fate::Kept, Proof::Proven, Some(w));
    }

    /// A read of the binding under the current parent, which does not read it.
    fn keep_used(&mut self, slot: Slot, child: &Expr) {
        self.keep(slot, |w| {
            Why::Use(match w.parent {
                Parent::Stmt(s) => Name::new(s),
                Parent::Expr(e) => {
                    let mut n = Name::of_expr(e);
                    n.push(store_role(e, child));
                    n
                }
            })
        });
    }

    fn keep_id(&mut self, id: LocalId, why: Why) {
        if let Some(s) = self.param(id) {
            self.keep(s, |_| why);
        }
    }

    fn stmts(&mut self, body: &'e [Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn opt(&mut self, e: &'e Option<Expr>) {
        if let Some(e) = e {
            self.expr(e);
        }
    }

    /// Walks `e` as a child of `parent`.
    fn sub(&mut self, parent: &'e Expr, e: &'e Expr) {
        let saved = std::mem::replace(&mut self.parent, Parent::Expr(parent));
        self.expr(e);
        self.parent = saved;
    }

    fn at(&mut self, s: &'static str) {
        self.parent = Parent::Stmt(s);
    }

    fn stmt(&mut self, stmt: &'e Stmt) {
        match stmt {
            Stmt::Return(Some(e)) => match self.slot(e) {
                Some(s) => self.raise(s, Fate::Returned, Proof::Proven),
                None => {
                    self.at("Return");
                    self.expr(e)
                }
            },
            Stmt::Return(None) => {}
            // A copy's declaration: the copy is the parameter.
            Stmt::Let {
                id,
                init: Some(Expr::LocalGet(_)),
                ..
            } if self.aliases.iter().any(|(q, _)| q == id) => {}
            Stmt::Let { init, .. } => {
                self.at("Let");
                self.opt(init)
            }
            Stmt::Expr(e) => {
                self.at("Expr");
                self.test(e)
            }
            Stmt::Throw(e) => {
                self.at("Throw");
                self.expr(e)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.at("If");
                self.test(condition);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.at("While");
                self.test(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(s) = init {
                    self.stmt(s);
                }
                self.at("For");
                if let Some(c) = condition {
                    self.test(c);
                }
                self.at("For");
                self.opt(update);
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
                    self.stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.at("Switch");
                self.expr(discriminant);
                for c in cases {
                    self.at("Switch");
                    self.opt(&c.test);
                    self.stmts(&c.body);
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

    /// `e` whose value is only tested for truthiness or discarded: a
    /// binding there drops (ToBoolean runs no code), and so does each value
    /// `&&`/`||`/`??`/`?:` can yield.
    fn test(&mut self, e: &'e Expr) {
        if let Some(s) = self.slot(e) {
            self.raise(s, Fate::Dropped, Proof::Proven);
            return;
        }
        match e {
            Expr::Logical { left, right, .. } => {
                self.test_sub(e, left);
                self.test_sub(e, right);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.test_sub(e, condition);
                self.test_sub(e, then_expr);
                self.test_sub(e, else_expr);
            }
            _ => self.expr(e),
        }
    }

    /// [`Self::test`] of `e` as a child of `parent`.
    fn test_sub(&mut self, parent: &'e Expr, e: &'e Expr) {
        let saved = std::mem::replace(&mut self.parent, Parent::Expr(parent));
        self.test(e);
        self.parent = saved;
    }

    /// `e`, the operand of an operator that converts it (and on an object
    /// calls its `valueOf`/`toString`/`Symbol.toPrimitive`): a binding the
    /// proof says holds only Numbers drops, anything else is walked.
    fn converted(&mut self, parent: &'e Expr, e: &'e Expr) {
        if let Expr::LocalGet(id) = e {
            if let Some(s) = self.param(*id) {
                if let Some(p) = self.proof.primitive(self.func, *id) {
                    self.raise(s, Fate::Dropped, p);
                    return;
                }
            }
        }
        self.sub(parent, e);
    }

    /// `value` assigned to the binding of `s`: what can flow into it from
    /// `s` itself stays `s`'s; anything else is walked as a use.
    fn into(&mut self, parent: &'e Expr, s: Slot, value: &'e Expr) {
        if self.slot(value) == Some(s) {
            return;
        }
        match value {
            Expr::Logical { left, right, .. } => {
                self.into(value, s, left);
                self.into(value, s, right);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.test_sub(value, condition);
                self.into(value, s, then_expr);
                self.into(value, s, else_expr);
            }
            _ => self.sub(parent, value),
        }
    }

    /// One use of `arg` as argument `i` of `call` whose position fate is
    /// `fate` (`why` when it keeps); anything but a tracked binding is
    /// walked as usual.
    fn argument(&mut self, call: &'e Expr, arg: &'e Expr, fate: ParamFate, why: Why) {
        match self.slot(arg) {
            Some(s) if fate.fate == Fate::Dropped => self.raise(s, Fate::Dropped, fate.proof),
            Some(s) => self.keep(s, |_| why),
            None => self.sub(call, arg),
        }
    }

    /// Why an argument of a call with these bodies is kept.
    fn known_why(&self, targets: &[FuncId], fate: Fate) -> Why {
        if targets.iter().any(|t| self.known.of_fn(*t).is_none()) {
            Why::ArgNoFate
        } else {
            Why::ArgKnown(fate)
        }
    }

    fn expr(&mut self, e: &'e Expr) {
        match e {
            Expr::LocalGet(id) => {
                if let Some(s) = self.param(*id) {
                    self.keep_used(s, e);
                }
            }
            Expr::This => self.keep_used(Slot::This, e),
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                ..
            } if self.slot(object).is_some() => {
                let s = self.slot(object).expect("tracked");
                match self.proof.read(self.func, object, property, *byte_offset) {
                    Some(p) => self.raise(s, Fate::Dropped, p),
                    None => self.keep(s, |_| Why::Read(Name::new(property))),
                }
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } if matches!(object.as_ref(), Expr::This) => {
                match self.store(property) {
                    Some(p) => match self.slot(value) {
                        Some(s) => self.raise(s, Fate::KeptByReceiver, p),
                        None => self.sub(e, value),
                    },
                    None => {
                        self.keep(Slot::This, |_| Why::Store(Name::new(property)));
                        self.sub(e, value);
                    }
                }
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This)
                && matches!(key.as_ref(), Expr::String(_)) =>
            {
                let Expr::String(name) = key.as_ref() else {
                    unreachable!("matched above")
                };
                match (self.store(name), self.slot(value)) {
                    (Some(p), Some(s)) => self.raise(s, Fate::KeptByReceiver, p),
                    (Some(_), None) => self.sub(e, value),
                    (None, _) => {
                        self.keep(Slot::This, |_| Why::Store(Name::new(name)));
                        self.sub(e, value);
                    }
                }
            }
            Expr::Compare { op, left, right } => {
                let address = match op {
                    CompareOp::Eq | CompareOp::Ne => true,
                    CompareOp::LooseEq | CompareOp::LooseNe => nullish(left) || nullish(right),
                    _ => false,
                };
                for side in [left, right] {
                    match self.slot(side) {
                        Some(s) if address => self.raise(s, Fate::Dropped, Proof::Proven),
                        _ => self.converted(e, side),
                    }
                }
            }
            Expr::TypeOf(x) => match self.slot(x) {
                Some(s) => self.raise(s, Fate::Dropped, Proof::Proven),
                None => self.sub(e, x),
            },
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => self.test_sub(e, operand),
            Expr::BooleanCoerce(x) => self.test_sub(e, x),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.test_sub(e, condition);
                self.sub(e, then_expr);
                self.sub(e, else_expr);
            }
            Expr::Binary { left, right, .. } => {
                self.converted(e, left);
                self.converted(e, right);
            }
            Expr::Unary { operand, .. }
            | Expr::TemplateStringCoerce(operand)
            | Expr::StringCoerce(operand)
            | Expr::NumberCoerce(operand) => self.converted(e, operand),
            Expr::Call { callee, args, .. } => {
                let targets = self.proof.targets(callee);
                // The callee itself: a method call keeps its receiver unless
                // the targets are known, and then the receiver is `this` of
                // those bodies.
                match (callee.as_ref(), &targets) {
                    (Expr::FuncRef(_), _) => {}
                    (Expr::PropertyGet { object, .. }, Some((ts, p)))
                        if self.slot(object).is_some() =>
                    {
                        let s = self.slot(object).expect("tracked");
                        let mut f = ParamFate::DROPPED;
                        for t in ts {
                            match self.known.of_fn(*t) {
                                Some(ff) => f.raise(ff.this.fate, ff.this.proof, None),
                                None => f.raise(Fate::Kept, Proof::Proven, None),
                            }
                        }
                        f.raise(Fate::Dropped, *p, None);
                        if f.fate == Fate::Dropped {
                            self.raise(s, Fate::Dropped, f.proof);
                        } else {
                            self.keep(s, |_| Why::ReceiverKept);
                        }
                    }
                    (Expr::PropertyGet { object, property, .. }, _) => match self.slot(object) {
                        Some(s) => self.keep(s, |_| Why::Receiver(Name::new(property))),
                        None => self.sub(callee, object),
                    },
                    (other, _) => self.sub(e, other),
                }
                for (i, a) in args.iter().enumerate() {
                    let (fate, why) = match &targets {
                        Some((ts, p)) => {
                            let mut f = self.known.call_arg(ts, i);
                            f.raise(Fate::Dropped, *p, None);
                            (f, self.known_why(ts, f.fate))
                        }
                        None => (ParamFate::KEPT, Why::ArgUnresolved(callee_name(callee))),
                    };
                    self.argument(e, a, fate, why);
                }
            }
            Expr::New {
                class_name, args, ..
            } => {
                for (i, a) in args.iter().enumerate() {
                    let fate = self.known.construction_arg(class_name, i);
                    let why = match self.known.construction.get(class_name.as_str()) {
                        Some(Some(ctor)) => self.known_why(&[*ctor], fate.fate),
                        Some(None) => Why::ArgKnown(fate.fate),
                        None => Why::NewUnknown(Name::new(class_name)),
                    };
                    self.argument(e, a, fate, why);
                }
            }
            Expr::Closure { captures_this, .. } => {
                let mut refs = Vec::new();
                perry_hir::analysis::collect_local_refs_expr(
                    e,
                    &mut refs,
                    &mut std::collections::HashSet::new(),
                );
                for id in refs {
                    self.keep_id(id, Why::Captured);
                }
                if *captures_this {
                    self.keep(Slot::This, |_| Why::CapturesThis);
                }
            }
            // The variants that name a local by id rather than through a
            // `LocalGet` child: a reassignment, `q++` (which runs `valueOf`),
            // an array or set mutation of it.
            Expr::LocalSet(id, value) => match self.param(*id) {
                Some(s) => self.into(e, s, value),
                None => self.sub(e, value),
            },
            Expr::Update { id, .. } => self.keep_id(*id, Why::Updated),
            Expr::ArrayPop(id) => self.keep_id(*id, Why::Mutated("ArrayPop")),
            Expr::ArrayShift(id) => self.keep_id(*id, Why::Mutated("ArrayShift")),
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. }
            | Expr::SetAdd {
                set_id: array_id, ..
            } => {
                let kind = match e {
                    Expr::ArrayPush { .. } => "ArrayPush",
                    Expr::ArrayPushSpread { .. } => "ArrayPushSpread",
                    Expr::ArrayUnshift { .. } => "ArrayUnshift",
                    Expr::ArraySplice { .. } => "ArraySplice",
                    Expr::ArrayCopyWithin { .. } => "ArrayCopyWithin",
                    _ => "SetAdd",
                };
                self.keep_id(*array_id, Why::Mutated(kind));
                self.children(e);
            }
            _ => self.children(e),
        }
    }

    fn children(&mut self, e: &'e Expr) {
        let mut kids = Vec::new();
        perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
        for c in kids {
            self.sub(e, c);
        }
    }
}

/// The never-written copies `let r = q` of a parameter (or of an earlier
/// copy) in `f`'s statement lists, outside closures: (copy, parameter
/// index). Never written: no `LocalSet`/`Update` of `r` anywhere in `f`,
/// its defaults or its closures (`local_occurrences`' writes census).
fn aliases(f: &Function) -> Vec<(LocalId, usize)> {
    fn scan(
        stmts: &[Stmt],
        f: &Function,
        writes: &HashMap<LocalId, usize>,
        out: &mut Vec<(LocalId, usize)>,
    ) {
        for s in stmts {
            match s {
                Stmt::Let {
                    id,
                    init: Some(Expr::LocalGet(p)),
                    ..
                } if !writes.contains_key(id) && f.params.iter().all(|q| q.id != *id) => {
                    let i = f
                        .params
                        .iter()
                        .position(|q| q.id == *p)
                        .or_else(|| out.iter().find(|(q, _)| q == p).map(|(_, i)| *i));
                    if let Some(i) = i {
                        out.push((*id, i));
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    scan(then_branch, f, writes, out);
                    if let Some(b) = else_branch {
                        scan(b, f, writes, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => scan(body, f, writes, out),
                Stmt::For { init, body, .. } => {
                    if let Some(s) = init {
                        scan(std::slice::from_ref(s.as_ref()), f, writes, out);
                    }
                    scan(body, f, writes, out);
                }
                Stmt::Labeled { body, .. } => scan(std::slice::from_ref(body.as_ref()), f, writes, out),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    scan(body, f, writes, out);
                    if let Some(c) = catch {
                        scan(&c.body, f, writes, out);
                    }
                    if let Some(b) = finally {
                        scan(b, f, writes, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        scan(&c.body, f, writes, out);
                    }
                }
                _ => {}
            }
        }
    }
    let writes = crate::local_occurrences::count_function_writes(f);
    let mut out = Vec::new();
    scan(&f.body, f, &writes, &mut out);
    out
}

/// Which part of a store `child` is: `.object` (stored into), `.value`
/// (stored into another object) or `.value-to-this` (into `this`, the store
/// not proven); empty for any other parent.
fn store_role(parent: &Expr, child: &Expr) -> &'static str {
    let (object, value) = match parent {
        Expr::PropertySet { object, value, .. } | Expr::IndexSet { object, value, .. } => {
            (object.as_ref(), value.as_ref())
        }
        Expr::PutValueSet { target, value, .. } => (target.as_ref(), value.as_ref()),
        _ => return "",
    };
    if std::ptr::eq(object, child) {
        ".object"
    } else if std::ptr::eq(value, child) {
        if matches!(object, Expr::This) {
            ".value-to-this"
        } else {
            ".value"
        }
    } else {
        ""
    }
}

/// The callee of an unresolved call, as a reason: its node, and for a
/// property callee the property (`PropertyGet.push`).
fn callee_name(callee: &Expr) -> Name {
    let mut n = Name::of_expr(callee);
    if let Expr::PropertyGet { property, .. } = callee {
        n.push(".");
        n.push(property);
    }
    n
}

fn nullish(e: &Expr) -> bool {
    matches!(e, Expr::Null | Expr::Undefined)
}

/// Every body a call or construction in `f` names, for the call graph.
fn callees(
    f: &Function,
    proof: &dyn FateProof,
    construction: &HashMap<String, Option<FuncId>>,
    out: &mut impl FnMut(FuncId),
) {
    let mut visit = |e: &Expr| {
        match e {
            Expr::Call { callee, .. } => {
                if let Some((ts, _)) = proof.targets(callee) {
                    ts.into_iter().for_each(&mut *out);
                }
            }
            Expr::New { class_name, .. } => {
                if let Some(Some(c)) = construction.get(class_name) {
                    out(*c);
                }
            }
            _ => {}
        }
        false
    };
    for d in f.params.iter().filter_map(|p| p.default.as_ref()) {
        each_expr(d, &mut visit);
    }
    for s in &f.body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            each_expr(e, &mut visit);
            false
        });
    }
}

fn each_expr<'a>(e: &'a Expr, f: &mut impl FnMut(&'a Expr) -> bool) {
    f(e);
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| each_expr(c, f));
}

/// Tarjan's strongly connected components, callees before callers.
fn sccs(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct T<'a> {
        edges: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    impl T<'_> {
        // Iterative: call graphs of real programs are deep.
        fn run(&mut self, root: usize) {
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];
            self.enter(root);
            while let Some(&(v, k)) = work.last() {
                if let Some(&w) = self.edges[v].get(k) {
                    work.last_mut().expect("top").1 += 1;
                    match self.index[w] {
                        None => {
                            self.enter(w);
                            work.push((w, 0));
                        }
                        Some(wi) if self.on[w] => self.low[v] = self.low[v].min(wi),
                        Some(_) => {}
                    }
                    continue;
                }
                work.pop();
                if let Some(&(u, _)) = work.last() {
                    self.low[u] = self.low[u].min(self.low[v]);
                }
                if Some(self.low[v]) == self.index[v] {
                    let mut c = Vec::new();
                    loop {
                        let w = self.stack.pop().expect("on stack");
                        self.on[w] = false;
                        c.push(w);
                        if w == v {
                            break;
                        }
                    }
                    self.out.push(c);
                }
            }
        }
        fn enter(&mut self, v: usize) {
            self.index[v] = Some(self.next);
            self.low[v] = self.next;
            self.next += 1;
            self.stack.push(v);
            self.on[v] = true;
        }
    }
    let n = edges.len();
    let mut t = T {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if t.index[v].is_none() {
            t.run(v);
        }
    }
    t.out
}

#[cfg(test)]
#[path = "effects_fate_tests.rs"]
mod tests;
