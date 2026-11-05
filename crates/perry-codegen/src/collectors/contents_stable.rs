//! Contents-stable bindings: the bindings whose object no code can write for
//! the binding's whole life.
//!
//! A proof about a binding's CONTENTS (an object literal's property types, the
//! descriptor a guarded clone checked at entry) is established once, at the
//! binding. It stays true only while nothing writes the object: a `delete`, a
//! store of another type or a `defineProperty` between the proof and a read
//! changes the property's value (`delete o.y; o.y * 10` is NaN, not the
//! Number the literal was born with). This is the one definition of "nothing
//! writes it", used by the let-binding proofs (`FnCtx::admits_contents_proof`)
//! and by the guarded parameter clones (`guard_blocked` in `compile_module`).
//!
//! The analysis is module-wide because a module-level binding is reachable
//! from every function of the module (local ids are module-unique). Aliases
//! (`const t = o`, `t = o`) form one class; a class is stable when no member
//! is
//! - written: a store, update or delete rooted at it (`o.x = v`, `o[i] = v`,
//!   `o.a.b = v`, `delete o.y`, `o.push(v)`), or reassigned;
//! - escaped: used as a value anywhere but a read receiver, an operand of an
//!   arithmetic/comparison operator, or `return` from its own frame. A call
//!   argument, a method receiver (`this`), a value stored into another object,
//!   a closure's value use, `with`, and every expression this walk does not
//!   list (`Object.assign`, `defineProperty`, spread, ...) are escapes. The one
//!   refinement: an argument to a known function whose parameter is itself
//!   contents-stable and never returned (that callee can neither write nor
//!   leak it).
//! A read of a member's property used as a value (`g(o.a)`, `const t = o.a`)
//! hands out the child object: it escapes the parent unless the field is
//! primitive-valued in every literal of the class.
//!
//! A class whose every source is a literal construction (or another member)
//! is FRESH: nothing outside can hold its object before the binding. Any other
//! class (a parameter, a call result, a `catch` binding) may already be
//! aliased by the caller, so it is stable only if additionally no body that
//! binds a member runs unknown code ([`body_contains_call`], #8094) or stores
//! through a root that is not a fresh member.
//!
//! Exported module bindings are never stable: another module can write them.

use std::collections::{HashMap, HashSet};

use perry_hir::types::FuncId;
use perry_hir::{Expr, Module, Param, Stmt};

use super::mutation::body_contains_call;

/// Every contents-stable binding id of `hir` (lets and parameters).
pub(crate) fn contents_stable_bindings(hir: &Module) -> HashSet<u32> {
    let mut scan = Scan::default();
    scan.body(&[], &hir.init);
    for f in &hir.functions {
        scan.body(&f.params, &f.body);
        scan.functions
            .insert(f.id, f.params.iter().map(|p| (p.id, p.is_rest)).collect());
    }
    for c in &hir.classes {
        let fields: Vec<String> = c.fields.iter().map(|f| f.name.clone()).collect();
        scan.class_fields.insert(c.name.clone(), fields);
        if let Some(ctor) = &c.constructor {
            scan.body(&ctor.params, &ctor.body);
        }
        for f in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            scan.body(&f.params, &f.body);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            let roots: Vec<&Expr> = field.key_expr.iter().chain(field.init.iter()).collect();
            if !roots.is_empty() {
                scan.begin_body(true);
                for e in roots {
                    scan.expr(e, Use::Value);
                }
                scan.end_body();
            }
        }
    }
    let exported: HashSet<&str> = hir
        .exports
        .iter()
        .filter_map(|e| match e {
            perry_hir::Export::Named { local, .. } => Some(local.as_str()),
            _ => None,
        })
        .collect();
    for s in &hir.init {
        if let Stmt::Let { id, name, .. } = s {
            if exported.contains(name.as_str()) {
                scan.written.insert(*id);
            }
        }
    }
    scan.resolve()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// The value is consumed where it stands (an operand, a discarded
    /// statement value): it reaches no other binding or heap slot.
    Consume,
    /// The value can reach code or storage this walk does not follow.
    Value,
    /// `return <e>`: harmless for a binding of the returning frame.
    Return,
}

#[derive(Default)]
struct Scan {
    parent: HashMap<u32, u32>,
    written: HashSet<u32>,
    /// Members bound by something other than a literal or another member.
    entry_aliased: HashSet<u32>,
    /// Literal initializers per member.
    fresh_inits: HashMap<u32, Vec<Expr>>,
    /// (parent member, field read) whose value was handed out; `None` is an
    /// index or a deeper path.
    child_escapes: Vec<(u32, Option<String>)>,
    /// (member, callee, argument index) of a known-function call argument.
    call_args: Vec<(u32, FuncId, usize)>,
    /// Parameter ids of each known function (`id`, `is_rest`).
    functions: HashMap<FuncId, Vec<(u32, bool)>>,
    class_fields: HashMap<String, Vec<String>>,
    returned: HashSet<u32>,
    /// (frame binding, field read) handed to the caller by `return o.f`
    /// (`None`: an index or a deeper path). A caller argument bound to that
    /// parameter has that child handed out.
    returned_children: Vec<(u32, Option<String>)>,
    /// The body (index) that binds each member.
    home: HashMap<u32, usize>,
    /// Per body: does it run unknown code.
    body_calls: Vec<bool>,
    /// Per body: the roots of its stores (`None`: not a local).
    store_roots: Vec<Vec<Option<u32>>>,
    current: usize,
    /// Bindings of the innermost function frame (for `return`).
    frames: Vec<HashSet<u32>>,
}

impl Scan {
    fn begin_body(&mut self, calls: bool) {
        self.current = self.body_calls.len();
        self.body_calls.push(calls);
        self.store_roots.push(Vec::new());
        self.frames.push(HashSet::new());
    }

    fn end_body(&mut self) {
        self.frames.pop();
    }

    fn body(&mut self, params: &[Param], body: &[Stmt]) {
        self.begin_body(body_contains_call(body));
        self.params(params);
        self.stmts(body);
        self.end_body();
    }

    fn params(&mut self, params: &[Param]) {
        for p in params {
            self.declare(p.id);
            self.entry_aliased.insert(p.id);
            if let Some(d) = &p.default {
                self.expr(d, Use::Value);
            }
        }
    }

    fn declare(&mut self, id: u32) {
        self.home.insert(id, self.current);
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(id);
        }
    }

    fn find(&mut self, id: u32) -> u32 {
        let mut root = id;
        while let Some(&p) = self.parent.get(&root) {
            if p == root {
                break;
            }
            root = p;
        }
        let mut cur = id;
        while cur != root {
            let next = self.parent.insert(cur, root).unwrap_or(root);
            cur = next;
        }
        root
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => self.bind(*id, init.as_ref()),
            Stmt::Expr(e) => self.expr(e, Use::Consume),
            Stmt::Throw(e) => self.expr(e, Use::Value),
            Stmt::Return(Some(e)) => self.expr(e, Use::Return),
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, Use::Consume);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, Use::Consume);
                self.stmts(body);
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
                    self.expr(c, Use::Consume);
                }
                if let Some(u) = update {
                    self.expr(u, Use::Consume);
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
                    if let Some((id, _)) = &c.param {
                        self.declare(*id);
                        self.entry_aliased.insert(*id);
                    }
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
                self.expr(discriminant, Use::Consume);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t, Use::Consume);
                    }
                    self.stmts(&c.body);
                }
            }
            Stmt::PreallocateBoxes(ids)
            | Stmt::PreallocateTdzBoxes(ids)
            | Stmt::ReleaseBoxes(ids) => {
                // A box starts as `undefined` and is filled by a later
                // assignment; the slot itself is not a value source.
                let _ = ids;
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        }
    }

    /// `id` is bound to `init` (a `let`, or a scoped temporary).
    fn bind(&mut self, id: u32, init: Option<&Expr>) {
        self.declare(id);
        match init {
            Some(Expr::LocalGet(other)) => self.union(id, *other),
            Some(e) if is_literal_construction(e) => {
                self.fresh_inits.entry(id).or_default().push(e.clone());
                if contains_closure(e) {
                    // A method or accessor of the literal runs with the
                    // object as `this`.
                    self.entry_aliased.insert(id);
                }
                self.expr_children(e, Use::Value);
            }
            Some(e) => {
                self.entry_aliased.insert(id);
                self.expr(e, Use::Value);
            }
            None => {
                self.entry_aliased.insert(id);
            }
        }
    }

    /// A store through `target` (the object whose slot is written).
    fn store(&mut self, target: &Expr) {
        match chain_root(target) {
            Some(root) => {
                self.written.insert(root);
                // A deeper store (`o.a.b = v`) writes a child, which need not
                // be fresh even when `o` is.
                let direct = matches!(target, Expr::LocalGet(_));
                self.store_roots[self.current].push(direct.then_some(root));
                self.chain_indices(target);
            }
            None => {
                self.store_roots[self.current].push(None);
                self.expr(target, Use::Value);
            }
        }
    }

    /// Index expressions along a read chain are consumed as keys.
    fn chain_indices(&mut self, e: &Expr) {
        match e {
            Expr::PropertyGet { object, .. } => self.chain_indices(object),
            Expr::IndexGet { object, index, .. } => {
                self.chain_indices(object);
                self.expr(index, Use::Consume);
            }
            _ => {}
        }
    }

    fn escape(&mut self, id: u32, how: Use) {
        if how == Use::Return && self.frames.last().is_some_and(|f| f.contains(&id)) {
            self.returned.insert(id);
        } else if how != Use::Consume {
            self.written.insert(id);
        }
    }

    /// A read receiver: the object is read, not handed out.
    fn receiver(&mut self, object: &Expr) {
        match object {
            Expr::LocalGet(_) => {}
            other => self.expr(other, Use::Consume),
        }
    }

    fn expr(&mut self, e: &Expr, how: Use) {
        match e {
            Expr::LocalGet(id) => self.escape(*id, how),
            Expr::PropertyGet {
                object, property, ..
            } => {
                self.receiver(object);
                if how != Use::Consume {
                    if let Some(root) = chain_root(object) {
                        let field =
                            matches!(object.as_ref(), Expr::LocalGet(_)).then(|| property.clone());
                        if how == Use::Return
                            && self.frames.last().is_some_and(|f| f.contains(&root))
                        {
                            self.returned_children.push((root, field));
                        } else {
                            self.child_escapes.push((root, field));
                        }
                    }
                }
            }
            Expr::IndexGet { object, index, .. } => {
                self.receiver(object);
                self.expr(index, Use::Consume);
                if how != Use::Consume {
                    if let Some(root) = chain_root(object) {
                        if how == Use::Return
                            && self.frames.last().is_some_and(|f| f.contains(&root))
                        {
                            self.returned_children.push((root, None));
                        } else {
                            self.child_escapes.push((root, None));
                        }
                    }
                }
            }
            Expr::LocalSet(id, value) => {
                self.written.insert(*id);
                match value.as_ref() {
                    Expr::LocalGet(other) => self.union(*id, *other),
                    v => self.expr(v, Use::Value),
                }
            }
            Expr::Update { id, .. } => {
                self.written.insert(*id);
            }
            Expr::ScopedTemp { id, value, body } => {
                self.bind(*id, Some(value));
                self.expr(body, how);
            }
            Expr::PropertySet { object, value, .. } => {
                self.store(object);
                self.expr(value, Use::Value);
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            } => {
                self.store(object);
                self.expr(index, Use::Consume);
                self.expr(value, Use::Value);
            }
            Expr::PropertyUpdate { object, .. } => self.store(object),
            Expr::IndexUpdate { object, index, .. } => {
                self.store(object);
                self.expr(index, Use::Consume);
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                self.store(target);
                self.store(receiver);
                self.expr(key, Use::Consume);
                self.expr(value, Use::Value);
            }
            Expr::Delete(operand) => match operand.as_ref() {
                Expr::PropertyGet { object, .. } => self.store(object),
                Expr::IndexGet { object, index, .. } => {
                    self.store(object);
                    self.expr(index, Use::Consume);
                }
                other => self.expr(other, Use::Consume),
            },
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                self.written.insert(*array_id);
                self.store_roots[self.current].push(Some(*array_id));
                self.expr_children(e, Use::Value);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                self.written.insert(*id);
                self.store_roots[self.current].push(Some(*id));
            }
            Expr::SetAdd { set_id, .. } => {
                self.written.insert(*set_id);
                self.store_roots[self.current].push(Some(*set_id));
                self.expr_children(e, Use::Value);
            }
            Expr::Call { callee, args, .. } => {
                match callee.as_ref() {
                    // A method call hands the receiver over as `this`.
                    Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
                        self.expr(object, Use::Value);
                        if let Expr::IndexGet { index, .. } = callee.as_ref() {
                            self.expr(index, Use::Consume);
                        }
                    }
                    other => self.expr(other, Use::Value),
                }
                let known = match callee.as_ref() {
                    Expr::FuncRef(fid) => Some(*fid),
                    _ => None,
                };
                for (i, arg) in args.iter().enumerate() {
                    match (known, arg) {
                        (Some(fid), Expr::LocalGet(id)) => self.call_args.push((*id, fid, i)),
                        _ => self.expr(arg, Use::Value),
                    }
                }
            }
            Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
                self.expr(left, Use::Consume);
                self.expr(right, Use::Consume);
            }
            Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
                self.expr(operand, Use::Consume)
            }
            Expr::Logical { left, right, .. } => {
                self.expr(left, how);
                self.expr(right, how);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition, Use::Consume);
                self.expr(then_expr, how);
                self.expr(else_expr, how);
            }
            Expr::Sequence(es) => {
                for (i, x) in es.iter().enumerate() {
                    self.expr(x, if i + 1 == es.len() { how } else { Use::Consume });
                }
            }
            Expr::Closure { params, body, .. } => {
                // A closure body runs whenever it is called: its uses count
                // as uses here, and its own bindings form a new frame.
                self.frames.push(HashSet::new());
                self.params(params);
                self.stmts(body);
                self.frames.pop();
            }
            _ => self.expr_children(e, Use::Value),
        }
    }

    fn expr_children(&mut self, e: &Expr, how: Use) {
        let mut children = Vec::new();
        perry_hir::walker::walk_expr_children(e, &mut |c| children.push(c));
        for c in children {
            self.expr(c, how);
        }
    }

    fn resolve(mut self) -> HashSet<u32> {
        let ids: Vec<u32> = self.home.keys().copied().collect();
        let mut fresh_class: HashMap<u32, bool> = HashMap::new();
        let mut class_inits: HashMap<u32, Vec<Expr>> = HashMap::new();
        for &id in &ids {
            let root = self.find(id);
            let entry = fresh_class.entry(root).or_insert(true);
            if self.entry_aliased.contains(&id) {
                *entry = false;
            }
            if let Some(inits) = self.fresh_inits.get(&id) {
                class_inits
                    .entry(root)
                    .or_default()
                    .extend(inits.iter().cloned());
            }
        }
        // A class needs a literal source to be fresh; an id never bound in
        // this module (a capture from elsewhere) is not.
        for (root, fresh) in fresh_class.iter_mut() {
            if !class_inits.contains_key(root) {
                *fresh = false;
            }
        }
        let is_fresh = |s: &mut Self, fresh: &HashMap<u32, bool>, id: u32| {
            let r = s.find(id);
            fresh.get(&r).copied().unwrap_or(false)
        };

        // A handed-out child escapes a fresh parent unless it is a primitive.
        // (An entry-aliased class needs a hazard-free body, and a write
        // through a handed-out child is a call or a store through a root
        // that is not a fresh member: a hazard.)
        let child_escapes = std::mem::take(&mut self.child_escapes);
        for (id, field) in child_escapes {
            let root = self.find(id);
            if !is_fresh(&mut self, &fresh_class, id) {
                continue;
            }
            let primitive = class_inits.get(&root).is_some_and(|inits| {
                inits.iter().all(|init| {
                    literal_field_is_primitive(init, field.as_deref(), &self.class_fields)
                })
            });
            if !primitive {
                self.written.insert(id);
            }
        }

        // Body hazards for entry-aliased classes: unknown code, or a store
        // through a root that is not a fresh member.
        let mut hazard: Vec<bool> = self.body_calls.clone();
        let store_roots = std::mem::take(&mut self.store_roots);
        for (body, roots) in store_roots.iter().enumerate() {
            if roots
                .iter()
                .any(|r| r.is_none_or(|id| !is_fresh(&mut self, &fresh_class, id)))
            {
                hazard[body] = true;
            }
        }

        let stable_now =
            |s: &mut Self, written_roots: &HashSet<u32>, hazard_roots: &HashSet<u32>, id: u32| {
                let r = s.find(id);
                !written_roots.contains(&r)
                    && (fresh_class.get(&r).copied().unwrap_or(false) || !hazard_roots.contains(&r))
            };
        let mut hazard_roots = HashSet::new();
        for &id in &ids {
            if self.home.get(&id).is_some_and(|b| hazard[*b]) {
                let r = self.find(id);
                hazard_roots.insert(r);
            }
        }
        let class_roots = |s: &mut Self| -> HashSet<u32> {
            let written: Vec<u32> = s.written.iter().copied().collect();
            written.into_iter().map(|w| s.find(w)).collect()
        };

        // Parameters never depend on the call-argument refinement: a body
        // that passes one on contains a call, which already blocks it.
        let written_roots = class_roots(&mut self);
        let call_args = std::mem::take(&mut self.call_args);
        let returned_children = std::mem::take(&mut self.returned_children);
        for (id, fid, index) in call_args {
            let callee_param = self
                .functions
                .get(&fid)
                .and_then(|params| params.get(index))
                .copied();
            let harmless = callee_param.is_some_and(|(param, is_rest)| {
                !is_rest
                    && !self.returned.contains(&param)
                    && stable_now(&mut self, &written_roots, &hazard_roots, param)
                    && returned_children
                        .iter()
                        .filter(|(p, _)| *p == param)
                        .all(|(_, field)| {
                            // `return p.f` hands the argument's child to the
                            // caller: harmless only for a primitive child.
                            let root = self.find(id);
                            fresh_class.get(&root).copied().unwrap_or(false)
                                && class_inits.get(&root).is_some_and(|inits| {
                                    inits.iter().all(|init| {
                                        literal_field_is_primitive(
                                            init,
                                            field.as_deref(),
                                            &self.class_fields,
                                        )
                                    })
                                })
                        })
            });
            if !harmless {
                self.written.insert(id);
            }
        }

        let written_roots = class_roots(&mut self);
        ids.into_iter()
            .filter(|&id| stable_now(&mut self, &written_roots, &hazard_roots, id))
            .collect()
    }
}

/// The local a read chain (`o`, `o.a`, `o[i].b`) starts from.
fn chain_root(e: &Expr) -> Option<u32> {
    match e {
        Expr::LocalGet(id) => Some(*id),
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => chain_root(object),
        _ => None,
    }
}

/// A construction that yields a new object no other binding holds.
fn is_literal_construction(e: &Expr) -> bool {
    match e {
        Expr::Object(_) | Expr::Array(..) => true,
        Expr::New { class_name, .. } => class_name.starts_with("__AnonShape_"),
        _ => false,
    }
}

fn contains_closure(e: &Expr) -> bool {
    if matches!(e, Expr::Closure { .. }) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |c| found = found || contains_closure(c));
    found
}

/// A value that can never be an object.
fn is_primitive_valued(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Binary { .. }
            | Expr::Compare { .. }
            | Expr::Unary { .. }
            | Expr::TypeOf(_)
    )
}

/// Is `field` (`None`: any field or element) of the literal `init` a
/// primitive? An absent field reads through the prototype: not known.
fn literal_field_is_primitive(
    init: &Expr,
    field: Option<&str>,
    class_fields: &HashMap<String, Vec<String>>,
) -> bool {
    let pairs: Vec<(&str, &Expr)> = match init {
        Expr::Object(props) => props.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        Expr::New {
            class_name, args, ..
        } => match class_fields.get(class_name) {
            Some(names) if names.len() == args.len() => {
                names.iter().map(String::as_str).zip(args.iter()).collect()
            }
            _ => return false,
        },
        Expr::Array(elements, _) => {
            return field.is_none() && elements.iter().all(is_primitive_valued);
        }
        _ => return false,
    };
    match field {
        Some(name) => pairs
            .iter()
            .find(|(k, _)| *k == name)
            .is_some_and(|(_, v)| is_primitive_valued(v)),
        None => pairs.iter().all(|(_, v)| is_primitive_valued(v)),
    }
}
