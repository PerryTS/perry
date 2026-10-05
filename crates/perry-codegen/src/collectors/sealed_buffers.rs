//! Sealed buffer bindings: which bindings can never hand their typed array
//! (or buffer) to code that reads its `.buffer`.
//!
//! A fresh `new Float64Array(n)` keeps its elements inline after the header
//! with a fixed length only while nobody observes its `.buffer`. The first
//! observation rebinds it to an external `ArrayBuffer` backing (the header's
//! storage byte flips to external, and the elements stop living at
//! `header + 16`), and `buffer.transfer()` / `transferToFixedLength()` /
//! structuredClone or postMessage with a transfer list then detaches it: the
//! length reads 0 and every element read is `undefined`. Code that caches the
//! data pointer, proves bounds against the construction length, or loads the
//! length as `!invariant.load` is sound only for an array whose `.buffer` can
//! never be observed. This pass proves that, syntactically, for a whole
//! module: a binding is SEALED when every use of it is one of
//!
//! - an element access receiver `x[k]` / `x[k] = v` / `x[k]++` (including the
//!   sloppy-mode `PutValueSet` whose target is also the receiver) whose key
//!   can never be a string (a string key such as `"buffer"` reaches the
//!   prototype getters);
//! - a `.length` read;
//! - a Buffer numeric read intrinsic (`x.readInt32BE(off)`);
//! - an argument of a direct call to a module function whose parameter at
//!   that position is itself sealed in the callee (a greatest fixed point, so
//!   recursion is fine);
//! - the initializer of `const alias = x` when the alias is itself sealed.
//!
//! Anything else (an alias, a method call, an unknown callee, a closure value
//! that escapes, an export, a reassignment) exposes the binding. The result is
//! the EXPOSED set; consumers combine "not exposed" with their own binding
//! proof (single immutable `let`, fresh construction).

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, Expr, Module, Stmt};

/// Which bindings some use could hand to code that reads their `.buffer`.
#[derive(Debug, Clone, Default)]
pub(crate) struct BufferExposure {
    /// Every exposed binding, plus every binding that is not a single
    /// never-reassigned `let` (or parameter).
    pub exposed: HashSet<u32>,
    /// The exposed bindings with an exposing use outside their own body (a
    /// closure, another function or class member), or a structural exposure
    /// (reassignment, export, a box, a hidden binding). Every other exposed
    /// binding is exposed only by statements of its own body, so the view may
    /// stay trusted until the first statement that may expose it
    /// ([`stmt_may_expose`]): that statement runs before anything after it,
    /// and a loop around both is a statement that contains it.
    pub remote: HashSet<u32>,
}

pub(crate) fn buffer_exposure(hir: &Module) -> BufferExposure {
    let mut scan = Scan::default();
    scan.walk_module(hir);
    let never_string = never_string_locals(hir, &scan);

    let mut exposed = HashSet::new();
    let mut remote = HashSet::new();
    let home = |id: u32| scan.let_home.get(&id).copied();
    let expose = |id: u32, body: u32, exposed: &mut HashSet<u32>, remote: &mut HashSet<u32>| {
        exposed.insert(id);
        if home(id) != Some(body) {
            remote.insert(id);
        }
    };
    for &(id, body) in &scan.events {
        expose(id, body, &mut exposed, &mut remote);
    }
    for (recv, keys, body) in &scan.keyed {
        if !keys.iter().all(|k| dep_holds(*k, &never_string, &scan)) {
            expose(*recv, *body, &mut exposed, &mut remote);
        }
    }
    let mut structural: HashSet<u32> = HashSet::new();
    structural.extend(scan.assigned.iter().copied());
    structural.extend(scan.opaque_bindings.iter().copied());
    structural.extend(scan.boxed.iter().copied());
    structural.extend(
        scan.let_counts
            .iter()
            .filter(|(_, count)| **count != 1)
            .map(|(id, _)| *id),
    );
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
                structural.insert(*id);
            }
        }
    }
    exposed.extend(structural.iter().copied());
    remote.extend(structural);

    // A call argument stays sealed only if the callee's parameter is sealed;
    // a `const alias = source` source only if the alias is.
    let functions: HashMap<u32, &perry_hir::Function> =
        hir.functions.iter().map(|f| (f.id, f)).collect();
    let sealed_param = |func: u32, pos: usize, exposed: &HashSet<u32>| -> bool {
        let Some(f) = functions.get(&func) else {
            return false;
        };
        if f.is_async || f.is_generator || f.params.iter().any(|p| p.arguments_object.is_some()) {
            return false;
        }
        let Some(p) = f.params.get(pos) else {
            return false;
        };
        !p.is_rest && p.decorators.is_empty() && !exposed.contains(&p.id)
    };
    loop {
        let mut changed = false;
        for &(arg, func, pos, body) in &scan.call_args {
            if !exposed.contains(&arg) && !sealed_param(func, pos, &exposed) {
                expose(arg, body, &mut exposed, &mut remote);
                changed = true;
            }
        }
        for &(source, alias, body) in &scan.aliases {
            if exposed.contains(&alias) && !exposed.contains(&source) {
                expose(source, body, &mut exposed, &mut remote);
                changed = true;
            }
            // An alias exposed from elsewhere exposes its source from there.
            if remote.contains(&alias) && exposed.contains(&source) && remote.insert(source) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    BufferExposure { exposed, remote }
}

/// Conservative per-statement probe for the late-exposed views of
/// [`BufferExposure`]: true when `stmt` (anywhere inside it, closures
/// included) uses `id` other than as an element receiver with a key that is
/// never a string on its face, a `.length` read or a Buffer read intrinsic.
/// It flags every exposing use the module scan found (and more: a key or a
/// call argument whose proof needs module facts counts as exposing here).
pub(crate) fn stmt_may_expose(stmt: &Stmt, id: u32) -> bool {
    let mut probe = Probe { id, found: false };
    probe.stmt(stmt);
    probe.found
}

struct Probe {
    id: u32,
    found: bool,
}

impl Probe {
    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        if self.found {
            return;
        }
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.expr(e),
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                if ids.contains(&self.id) {
                    self.found = true;
                }
            }
            Stmt::ReleaseBoxes(_)
            | Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(eb) = else_branch {
                    self.stmts(eb);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition);
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
                    self.expr(c);
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
                self.expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t);
                    }
                    self.stmts(&case.body);
                }
            }
        }
    }

    /// An element receiver: safe only with a dependency-free never-string key.
    fn receiver(&mut self, object: &Expr, key: &Expr) {
        if matches!(object, Expr::LocalGet(r) if *r == self.id) {
            let mut deps = Vec::new();
            if !key_never_string_shape(key, &mut deps) || !deps.is_empty() {
                self.found = true;
            }
        } else {
            self.expr(object);
        }
        self.expr(key);
    }

    fn expr(&mut self, e: &Expr) {
        if self.found {
            return;
        }
        match e {
            Expr::LocalGet(id) => {
                if *id == self.id {
                    self.found = true;
                }
            }
            Expr::IndexGet { object, index }
            | Expr::IndexUpdate { object, index, .. }
            | Expr::BufferIndexGet {
                buffer: object,
                index,
            }
            | Expr::Uint8ArrayGet {
                array: object,
                index,
            } => self.receiver(object, index),
            Expr::IndexSet {
                object,
                index,
                value,
            }
            | Expr::BufferIndexSet {
                buffer: object,
                index,
                value,
            }
            | Expr::Uint8ArraySet {
                array: object,
                index,
                value,
            } => {
                self.receiver(object, index);
                self.expr(value);
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(
                (target.as_ref(), receiver.as_ref()),
                (Expr::LocalGet(t), Expr::LocalGet(r)) if t == r
            ) =>
            {
                self.receiver(target, key);
                self.expr(value);
            }
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" && matches!(object.as_ref(), Expr::LocalGet(_)) => {}
            Expr::Uint8ArrayLength(array) if matches!(array.as_ref(), Expr::LocalGet(_)) => {}
            Expr::Call { callee, args, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, property, .. }
                        if matches!(object.as_ref(), Expr::LocalGet(_))
                            && crate::lower_call::buffer_intrinsic::is_buffer_numeric_read_method(
                                property,
                            )
                ) =>
            {
                for arg in args {
                    self.expr(arg);
                }
            }
            Expr::Closure { body, .. } => {
                self.stmts(body);
                perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
            }
            _ => perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c)),
        }
    }
}

#[derive(Default)]
struct Scan<'a> {
    /// The body being walked: the module init, a function, a class member or
    /// a closure each get their own id.
    body: u32,
    next_body: u32,
    /// The body each `let` binding lives in.
    let_home: HashMap<u32, u32>,
    /// `(local, body)`: a use in a position that may hand the value to
    /// arbitrary code.
    events: Vec<(u32, u32)>,
    /// `(receiver, key deps, body)`: an element access whose key is never a
    /// string iff every listed dependency holds.
    keyed: Vec<(u32, Vec<KeyDep>, u32)>,
    /// Locals whose `let` initializer is a fresh typed array or buffer: an
    /// element read through one (with a never-string key) is a Number, a
    /// BigInt or `undefined`, never a string.
    fresh_arrays: HashSet<u32>,
    /// `(arg local, callee FuncId, position, body)` for direct module-function
    /// calls.
    call_args: Vec<(u32, u32, usize, u32)>,
    /// Reassigned locals (`LocalSet` / `GlobalSet` / `Update` / `with` fallback).
    assigned: HashSet<u32>,
    /// Every value a local receives through a `let` initializer or assignment.
    writes: HashMap<u32, Vec<&'a Expr>>,
    /// Locals bound by something this scan cannot see the value of: closure,
    /// method, accessor and constructor parameters, `catch` bindings, `with`
    /// fallback targets.
    opaque_bindings: HashSet<u32>,
    let_counts: HashMap<u32, u32>,
    /// `PreallocateBoxes` / `PreallocateTdzBoxes` ids: the slot holds a box.
    boxed: HashSet<u32>,
    /// Module functions referenced other than as the callee of a direct call.
    func_values: HashSet<u32>,
    /// Argument lists of every direct call, per callee.
    call_sites: HashMap<u32, Vec<&'a [Expr]>>,
    /// `(source, alias, body)` for `const alias = source`: the source stays
    /// sealed only if the alias does.
    aliases: Vec<(u32, u32, u32)>,
}

impl<'a> Scan<'a> {
    /// Enter a fresh body; returns the one to restore.
    fn enter_body(&mut self) -> u32 {
        self.next_body += 1;
        std::mem::replace(&mut self.body, self.next_body)
    }

    fn expose(&mut self, id: u32) {
        self.events.push((id, self.body));
    }

    fn walk_module(&mut self, hir: &'a Module) {
        self.walk_stmts(&hir.init);
        for f in &hir.functions {
            let outer = self.enter_body();
            for p in &f.params {
                if let Some(d) = &p.default {
                    self.walk_expr(d);
                }
            }
            self.walk_stmts(&f.body);
            self.body = outer;
        }
        for c in &hir.classes {
            let mut members: Vec<&'a perry_hir::Function> = Vec::new();
            if let Some(ctor) = &c.constructor {
                members.push(ctor);
            }
            members.extend(c.methods.iter().chain(c.static_methods.iter()));
            members.extend(c.getters.iter().map(|(_, g)| g));
            members.extend(c.setters.iter().map(|(_, s)| s));
            members.extend(c.computed_members.iter().map(|m| &m.function));
            for m in members {
                let outer = self.enter_body();
                self.opaque_params(&m.params);
                self.walk_stmts(&m.body);
                self.body = outer;
            }
            let outer = self.enter_body();
            for m in &c.computed_members {
                self.walk_expr(&m.key_expr);
            }
            for field in c.fields.iter().chain(c.static_fields.iter()) {
                if let Some(key) = &field.key_expr {
                    self.walk_expr(key);
                }
                if let Some(init) = &field.init {
                    self.walk_expr(init);
                }
            }
            self.body = outer;
        }
    }

    fn opaque_params(&mut self, params: &'a [perry_hir::Param]) {
        for p in params {
            self.opaque_bindings.insert(p.id);
            if let Some(d) = &p.default {
                self.walk_expr(d);
            }
        }
    }

    fn walk_stmts(&mut self, stmts: &'a [Stmt]) {
        for s in stmts {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                *self.let_counts.entry(*id).or_insert(0) += 1;
                self.let_home.insert(*id, self.body);
                if matches!(
                    init,
                    Some(
                        Expr::TypedArrayNew { .. }
                            | Expr::Uint8ArrayNew(_)
                            | Expr::BufferAlloc { .. }
                            | Expr::BufferAllocUnsafe(_)
                    )
                ) {
                    self.fresh_arrays.insert(*id);
                }
                if let Some(e) = init {
                    self.writes.entry(*id).or_default().push(e);
                    if let Expr::LocalGet(source) = e {
                        self.aliases.push((*source, *id, self.body));
                    } else {
                        self.walk_expr(e);
                    }
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.walk_expr(e),
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                self.boxed.extend(ids.iter().copied());
            }
            Stmt::ReleaseBoxes(_)
            | Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
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
                if let Some(i) = init {
                    self.walk_stmt(i);
                }
                if let Some(c) = condition {
                    self.walk_expr(c);
                }
                if let Some(u) = update {
                    self.walk_expr(u);
                }
                self.walk_stmts(body);
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_stmts(body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        self.opaque_bindings.insert(*id);
                    }
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
        }
    }

    fn walk_children(&mut self, e: &'a Expr) {
        perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
    }

    /// An element access on `recv`: sealed only if the key is never a string.
    fn note_key(&mut self, recv: u32, key: &'a Expr) {
        let mut deps = Vec::new();
        if key_never_string_shape(key, &mut deps) {
            if !deps.is_empty() {
                self.keyed.push((recv, deps, self.body));
            }
        } else {
            self.expose(recv);
        }
    }

    fn walk_expr(&mut self, e: &'a Expr) {
        match e {
            Expr::LocalGet(id) => self.expose(*id),
            Expr::FuncRef(f) => {
                self.func_values.insert(*f);
            }
            Expr::LocalSet(id, value) | Expr::GlobalSet(id, value) => {
                self.assigned.insert(*id);
                self.writes.entry(*id).or_default().push(value);
                self.walk_expr(value);
            }
            Expr::Update { id, .. } => {
                self.assigned.insert(*id);
                self.walk_children(e);
            }
            Expr::WithSet { fallback, .. } => {
                if let perry_hir::WithSetFallback::Local(id)
                | perry_hir::WithSetFallback::SloppyImplicit(id) = fallback
                {
                    self.assigned.insert(*id);
                    self.opaque_bindings.insert(*id);
                }
                self.walk_children(e);
            }
            Expr::IndexGet { object, index } => {
                if let Expr::LocalGet(recv) = object.as_ref() {
                    self.note_key(*recv, index);
                } else {
                    self.walk_expr(object);
                }
                self.walk_expr(index);
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                if let Expr::LocalGet(recv) = object.as_ref() {
                    self.note_key(*recv, index);
                } else {
                    self.walk_expr(object);
                }
                self.walk_expr(index);
                self.walk_expr(value);
            }
            // Sloppy-mode element store: an ordinary [[Set]] whose target is
            // also the receiver. With a never-string key on a typed array it
            // is an element store (integer-indexed keys never reach the
            // prototype chain).
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(
                (target.as_ref(), receiver.as_ref()),
                (Expr::LocalGet(t), Expr::LocalGet(r)) if t == r
            ) =>
            {
                if let Expr::LocalGet(recv) = target.as_ref() {
                    self.note_key(*recv, key);
                }
                self.walk_expr(key);
                self.walk_expr(value);
            }
            Expr::IndexUpdate { object, index, .. } => {
                if let Expr::LocalGet(recv) = object.as_ref() {
                    self.note_key(*recv, index);
                } else {
                    self.walk_expr(object);
                }
                self.walk_expr(index);
            }
            Expr::BufferIndexGet {
                buffer: array,
                index,
            }
            | Expr::Uint8ArrayGet { array, index } => {
                if let Expr::LocalGet(recv) = array.as_ref() {
                    self.note_key(*recv, index);
                } else {
                    self.walk_expr(array);
                }
                self.walk_expr(index);
            }
            Expr::BufferIndexSet {
                buffer: array,
                index,
                value,
            }
            | Expr::Uint8ArraySet {
                array,
                index,
                value,
            } => {
                if let Expr::LocalGet(recv) = array.as_ref() {
                    self.note_key(*recv, index);
                } else {
                    self.walk_expr(array);
                }
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Expr::Uint8ArrayLength(array) if matches!(array.as_ref(), Expr::LocalGet(_)) => {}
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" && matches!(object.as_ref(), Expr::LocalGet(_)) => {}
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                Expr::FuncRef(f) => {
                    self.call_sites.entry(*f).or_default().push(args);
                    for (pos, arg) in args.iter().enumerate() {
                        if let Expr::LocalGet(id) = arg {
                            self.call_args.push((*id, *f, pos, self.body));
                        } else {
                            self.walk_expr(arg);
                        }
                    }
                }
                Expr::PropertyGet {
                    object, property, ..
                } if matches!(object.as_ref(), Expr::LocalGet(_))
                    && crate::lower_call::buffer_intrinsic::is_buffer_numeric_read_method(
                        property,
                    ) =>
                {
                    for arg in args {
                        self.walk_expr(arg);
                    }
                }
                _ => self.walk_children(e),
            },
            Expr::Closure { params, body, .. } => {
                let outer = self.enter_body();
                self.opaque_params(params);
                self.walk_stmts(body);
                // Other child expressions (parameter defaults again — harmless).
                self.walk_children(e);
                self.body = outer;
            }
            _ => self.walk_children(e),
        }
    }
}

/// What a never-string key shape still depends on.
#[derive(Clone, Copy)]
enum KeyDep {
    /// This local must never hold a string.
    Local(u32),
    /// This local must be a never-reassigned fresh typed array or buffer.
    Array(u32),
}

fn dep_holds(dep: KeyDep, never_string: &HashSet<u32>, scan: &Scan<'_>) -> bool {
    match dep {
        KeyDep::Local(id) => never_string.contains(&id),
        KeyDep::Array(id) => {
            scan.fresh_arrays.contains(&id)
                && scan.let_counts.get(&id).copied() == Some(1)
                && !scan.assigned.contains(&id)
                && !scan.opaque_bindings.contains(&id)
        }
    }
}

/// Structural half of "this key is never a string": numeric literals,
/// operators whose result is a Number, BigInt or Boolean, `+` of two such
/// operands, the numeric `Math` intrinsics, and an element read of a fresh
/// typed array with such a key. What remains to decide is collected in `deps`.
fn key_never_string_shape(e: &Expr, deps: &mut Vec<KeyDep>) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        // A canonical numeric string ("0", "17") is an element key too.
        Expr::String(s) => is_canonical_index_string(s),
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => key_never_string_shape(left, deps) && key_never_string_shape(right, deps),
        Expr::Binary { .. } | Expr::Unary { .. } | Expr::Compare { .. } | Expr::Update { .. } => {
            true
        }
        Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathAbs(_)
        | Expr::MathSign(_)
        | Expr::MathImul(..)
        | Expr::MathClz32(_)
        | Expr::MathMin(_)
        | Expr::MathMax(_) => true,
        Expr::IndexGet { object, index }
        | Expr::Uint8ArrayGet {
            array: object,
            index,
        }
        | Expr::BufferIndexGet {
            buffer: object,
            index,
        } => match object.as_ref() {
            Expr::LocalGet(arr) => {
                deps.push(KeyDep::Array(*arr));
                key_never_string_shape(index, deps)
            }
            _ => false,
        },
        Expr::LocalGet(id) => {
            deps.push(KeyDep::Local(*id));
            true
        }
        _ => false,
    }
}

/// `"0"` or a decimal integer without a leading zero: the strings that
/// `ToString(ToNumber(s))` maps back to themselves and that a typed array
/// therefore treats as an element index, never a property name.
fn is_canonical_index_string(s: &str) -> bool {
    s == "0"
        || (!s.is_empty()
            && s.len() <= 15
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Locals that never hold a string: every value they receive has a
/// never-string shape (greatest fixed point). A module function's parameter
/// qualifies when the function is only ever called directly (never used as a
/// value, never exported, no `arguments` object) and every call passes a
/// never-string argument at that position.
fn never_string_locals(hir: &Module, scan: &Scan<'_>) -> HashSet<u32> {
    let mut params: HashMap<u32, (u32, usize)> = HashMap::new();
    let mut candidates: HashSet<u32> = HashSet::new();
    for f in &hir.functions {
        let direct_only = !scan.func_values.contains(&f.id)
            && !f.is_exported
            && !f.params.iter().any(|p| p.arguments_object.is_some());
        for (pos, p) in f.params.iter().enumerate() {
            if direct_only && !p.is_rest && p.default.is_none() {
                params.insert(p.id, (f.id, pos));
                candidates.insert(p.id);
            }
        }
    }
    candidates.extend(scan.writes.keys().copied());
    candidates.extend(scan.let_counts.keys().copied());
    candidates.retain(|id| !scan.opaque_bindings.contains(id));
    let mut ns = candidates;
    let holds = |e: &Expr, ns: &HashSet<u32>| {
        let mut deps = Vec::new();
        key_never_string_shape(e, &mut deps) && deps.iter().all(|d| dep_holds(*d, ns, scan))
    };
    loop {
        let mut drop = Vec::new();
        for id in &ns {
            let writes_ok = scan
                .writes
                .get(id)
                .is_none_or(|ws| ws.iter().all(|w| holds(w, &ns)));
            let args_ok = params.get(id).is_none_or(|(func, pos)| {
                scan.call_sites.get(func).is_none_or(|sites| {
                    sites
                        .iter()
                        .all(|args| args.get(*pos).is_none_or(|a| holds(a, &ns)))
                })
            });
            if !writes_ok || !args_ok {
                drop.push(*id);
            }
        }
        if drop.is_empty() {
            break;
        }
        for id in drop {
            ns.remove(&id);
        }
    }
    ns
}

#[cfg(test)]
#[path = "sealed_buffers_tests.rs"]
mod tests;
