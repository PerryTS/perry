//! Forward a value stored into a field of a local object to the reads of
//! that field that follow the store, then drop the store.
//!
//! The inliner turns `put(b, new P(..))` with `function put(b, p) { b.p = p }`
//! into `let p = new P(..); b.p = p;` inside the caller's loop. Then `p` is
//! handed to another object, so it stays on the heap even when `b` dies in
//! the same iteration and `b.p` is only read back. When the store's value is
//! known and nothing but the statements of one list can touch `b`, each later
//! `b.p` is `p` itself, and once those reads are gone the store has no reader.
//!
//! For `let b = new C(..)` in a statement list `L` the rule asks:
//!
//! * `b` is never written, and every occurrence of `b` in the module (the
//!   census counts captures, box preallocations and intrinsics too) is in a
//!   statement of `L` after the declaration, as a read `b.f` or a store
//!   `b.f = v` to a field `C` declares. Anything else (a call on `b`, a bare
//!   `b`, a capture, a use from another body) may let other code see or
//!   change `b`'s fields, and leaves `b` alone. A read used as a callee is not
//!   a read of a value: it also passes `b` as `this`;
//! * `new C(..)` is closed (`crate::effects`): the construction keeps no
//!   `this` and runs no code that could freeze `b` or redefine a field;
//! * `f` is a field `C` defines at birth, with no getter or setter of `C` for
//!   its name: after construction `b.f` is an own writable data property, so
//!   a store to it runs no code and cannot throw, and a read gives back the
//!   value last stored;
//! * `f` is stored exactly once, by a statement of `L` itself (not inside a
//!   branch, loop, `try` or closure), so every later statement of `L` runs
//!   after it and no other store can intervene;
//! * no read of `f` comes before that store in `L`: such a read sees the
//!   field's initial value, and the store must stay for nobody;
//! * the stored value is a literal or a local the census shows is never
//!   written, declared earlier in `L` or in an enclosing list of the same
//!   body (so it is initialized at the store and in scope at every later
//!   read), of a type that binds by reference (see `scoped_copies::Census`):
//!   the read then denotes the same value, the same object for an object.
//!
//! Then every read of `f` after the store becomes the stored value, and the
//! store, whose value has no effect, has no reader left and is removed.
//! Top-level module-init declarations are registered globals and are left
//! alone; lists nested inside them are processed, unless the module awaits at
//! top level. Async and generator bodies are not processed.

use std::collections::{HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Expr, Function, Module, Stmt};

use crate::effects::{has_accessor, Effects};
use crate::scoped_copies::Census;

struct Cx {
    census: Census,
    /// For each class whose construction is closed: the fields that are own
    /// data properties from birth with no accessor of the class.
    own_fields: HashMap<String, HashSet<String>>,
}

/// Unwritten locals declared earlier in the enclosing lists, innermost last.
type Avail = Vec<(LocalId, Type)>;

pub fn run(module: &mut Module) {
    let effects = Effects::of_module(module);
    let own_fields = module
        .classes
        .iter()
        .filter(|class| effects.construction(&class.name).is_some())
        .map(|class| {
            let names = class
                .fields
                .iter()
                .filter(|f| {
                    f.origin.is_definition()
                        && f.key_expr.is_none()
                        && !has_accessor(class, &f.name)
                })
                .map(|f| f.name.clone())
                .collect();
            (class.name.clone(), names)
        })
        .collect();
    let cx = Cx {
        census: Census::of_module(module),
        own_fields,
    };
    for f in &mut module.functions {
        forward_function(f, &cx);
    }
    for class in &mut module.classes {
        for f in class
            .constructor
            .iter_mut()
            .chain(&mut class.methods)
            .chain(&mut class.static_methods)
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
            .chain(class.computed_members.iter_mut().map(|m| &mut m.function))
        {
            forward_function(f, &cx);
        }
    }
    if !module.has_top_level_await {
        let mut avail = Avail::new();
        for stmt in &mut module.init {
            forward_nested(stmt, &mut avail, &cx);
        }
    }
}

fn forward_function(f: &mut Function, cx: &Cx) {
    if !f.is_async && !f.is_generator {
        forward_list(&mut f.body, &mut Avail::new(), cx);
    }
}

fn forward_list(list: &mut Vec<Stmt>, avail: &mut Avail, cx: &Cx) {
    let base = avail.len();
    let mut i = 0;
    while i < list.len() {
        if let Some((b, own)) = owner_at(&list[i], cx) {
            forward_owner(list, i, b, own, avail, cx);
        }
        forward_nested(&mut list[i], avail, cx);
        if let Stmt::Let { id, ty, .. } = &list[i] {
            if cx.census.unwritten(*id) {
                avail.push((*id, ty.clone()));
            }
        }
        i += 1;
    }
    avail.truncate(base);
}

/// `let b = new C(..)` with `C` closed, as `b` and the fields of `C`.
fn owner_at<'c>(stmt: &Stmt, cx: &'c Cx) -> Option<(LocalId, &'c HashSet<String>)> {
    let Stmt::Let {
        id,
        init: Some(Expr::New { class_name, .. }),
        ..
    } = stmt
    else {
        return None;
    };
    let own = cx.own_fields.get(class_name)?;
    cx.census.unwritten(*id).then_some((*id, own))
}

/// What the statements after `b`'s declaration do with `b`.
#[derive(Default)]
struct Uses<'a> {
    /// Occurrences of `b`, to compare with the module census.
    count: usize,
    /// An occurrence that is not a read or store of an own field.
    other: bool,
    fields: HashMap<&'a str, FieldUses>,
}

#[derive(Default)]
struct FieldUses {
    stores: usize,
    /// Index in the list of the first statement that reads the field.
    first_read: Option<usize>,
}

fn forward_owner(
    list: &mut Vec<Stmt>,
    at: usize,
    b: LocalId,
    own: &HashSet<String>,
    avail: &Avail,
    cx: &Cx,
) {
    let mut done = Vec::new();
    {
        let mut uses = Uses::default();
        for (j, stmt) in list.iter().enumerate().skip(at + 1) {
            each_expr(stmt, &mut |e| scan_expr(e, b, own, j, &mut uses));
        }
        if uses.other || cx.census.all.get(&b).copied() != Some(uses.count + 1) {
            return;
        }
        for j in at + 1..list.len() {
            let Stmt::Expr(e) = &list[j] else { continue };
            let Some((f, value)) = field_store(e, b) else {
                continue;
            };
            let once = uses
                .fields
                .get(f)
                .is_some_and(|u| u.stores == 1 && u.first_read.is_none_or(|r| r > j));
            if once && forwardable(value, &list[..j], avail, cx) {
                done.push((j, f.to_string(), value.clone()));
            }
        }
    }
    for (j, f, value) in done.into_iter().rev() {
        for stmt in &mut list[j + 1..] {
            each_expr_mut(stmt, &mut |e| replace_reads(e, b, &f, &value));
        }
        list.remove(j);
    }
}

fn is_local(expr: &Expr, b: LocalId) -> bool {
    matches!(expr, Expr::LocalGet(id) if *id == b)
}

/// `b.<f>` as `f`.
fn field_read(expr: &Expr, b: LocalId) -> Option<&str> {
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } if is_local(object, b) => Some(property),
        _ => None,
    }
}

/// `b.<f> = <value>` as `(f, value)`.
fn field_store(expr: &Expr, b: LocalId) -> Option<(&str, &Expr)> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if is_local(object, b) => Some((property, value)),
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } if is_local(target, b) && is_local(receiver, b) => match key.as_ref() {
            Expr::String(name) => Some((name, value)),
            _ => None,
        },
        _ => None,
    }
}

fn scan_expr<'a>(
    expr: &'a Expr,
    b: LocalId,
    own: &HashSet<String>,
    at: usize,
    uses: &mut Uses<'a>,
) {
    if let Some(f) = field_read(expr, b) {
        uses.count += 1;
        if own.contains(f) {
            let u = uses.fields.entry(f).or_default();
            u.first_read.get_or_insert(at);
        } else {
            uses.other = true;
        }
        return;
    }
    if let Some((f, value)) = field_store(expr, b) {
        uses.count += if matches!(expr, Expr::PutValueSet { .. }) {
            2
        } else {
            1
        };
        if own.contains(f) {
            uses.fields.entry(f).or_default().stores += 1;
        } else {
            uses.other = true;
        }
        scan_expr(value, b, own, at, uses);
        return;
    }
    match expr {
        Expr::LocalGet(id) if *id == b => {
            uses.count += 1;
            uses.other = true;
            return;
        }
        Expr::Call { callee, .. }
        | Expr::CallSpread { callee, .. }
        | Expr::JsCallValue { callee, .. }
            if field_read(callee, b).is_some() =>
        {
            uses.other = true;
        }
        Expr::Delete(operand) if field_read(operand, b).is_some() => uses.other = true,
        _ => {}
    }
    walk_expr_children(expr, &mut |e| scan_expr(e, b, own, at, uses));
}

/// A literal, or a local initialized before `L[j]` that is never written.
fn forwardable(value: &Expr, before: &[Stmt], avail: &Avail, cx: &Cx) -> bool {
    match value {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_) => true,
        Expr::LocalGet(q) => {
            let here = before.iter().find_map(|s| match s {
                Stmt::Let { id, ty, .. } if id == q => Some(ty),
                _ => None,
            });
            let ty = here.or_else(|| avail.iter().rev().find(|(id, _)| id == q).map(|(_, ty)| ty));
            cx.census.unwritten(*q) && ty.is_some_and(|ty| cx.census.binds_by_reference(ty))
        }
        _ => false,
    }
}

fn replace_reads(expr: &mut Expr, b: LocalId, f: &str, value: &Expr) {
    if field_read(expr, b) == Some(f) {
        *expr = value.clone();
        return;
    }
    walk_expr_children_mut(expr, &mut |e| replace_reads(e, b, f, value));
}

/// Processes the lists and closure bodies inside one statement.
fn forward_nested(stmt: &mut Stmt, avail: &mut Avail, cx: &Cx) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                forward_expr(e, cx);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => forward_expr(e, cx),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            forward_expr(condition, cx);
            forward_list(then_branch, avail, cx);
            if let Some(b) = else_branch {
                forward_list(b, avail, cx);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            forward_expr(condition, cx);
            forward_list(body, avail, cx);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                forward_nested(i, avail, cx);
            }
            if let Some(e) = condition {
                forward_expr(e, cx);
            }
            if let Some(e) = update {
                forward_expr(e, cx);
            }
            forward_list(body, avail, cx);
        }
        Stmt::Labeled { body, .. } => forward_nested(body, avail, cx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            forward_list(body, avail, cx);
            if let Some(catch) = catch {
                forward_list(&mut catch.body, avail, cx);
            }
            if let Some(f) = finally {
                forward_list(f, avail, cx);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            forward_expr(discriminant, cx);
            for case in cases {
                if let Some(t) = &mut case.test {
                    forward_expr(t, cx);
                }
                forward_list(&mut case.body, avail, cx);
            }
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
    }
}

/// A closure body is its own function body: nothing outside it is known to
/// be initialized when it runs.
fn forward_expr(expr: &mut Expr, cx: &Cx) {
    if let Expr::Closure {
        body,
        is_async: false,
        is_generator: false,
        ..
    } = expr
    {
        forward_list(body, &mut Avail::new(), cx);
    }
    walk_expr_children_mut(expr, &mut |e| forward_expr(e, cx));
}

/// Calls `f` on every expression of `stmt` and of the statements inside it
/// (not inside closure bodies: the walker does not enter them).
fn each_expr<'a>(stmt: &'a Stmt, f: &mut dyn FnMut(&'a Expr)) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => init.iter().for_each(|e| f(e)),
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            then_branch
                .iter()
                .chain(else_branch.iter().flatten())
                .for_each(|s| each_expr(s, f));
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            body.iter().for_each(|s| each_expr(s, f));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                each_expr(s, f);
            }
            condition.iter().chain(update.iter()).for_each(|e| f(e));
            body.iter().for_each(|s| each_expr(s, f));
        }
        Stmt::Labeled { body, .. } => each_expr(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => body
            .iter()
            .chain(catch.iter().flat_map(|c| &c.body))
            .chain(finally.iter().flatten())
            .for_each(|s| each_expr(s, f)),
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for case in cases {
                case.test.iter().for_each(|e| f(e));
                case.body.iter().for_each(|s| each_expr(s, f));
            }
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
    }
}

/// [`each_expr`] for rewriting.
fn each_expr_mut(stmt: &mut Stmt, f: &mut dyn FnMut(&mut Expr)) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => init.iter_mut().for_each(|e| f(e)),
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            then_branch
                .iter_mut()
                .chain(else_branch.iter_mut().flatten())
                .for_each(|s| each_expr_mut(s, f));
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            body.iter_mut().for_each(|s| each_expr_mut(s, f));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                each_expr_mut(s, f);
            }
            condition
                .iter_mut()
                .chain(update.iter_mut())
                .for_each(|e| f(e));
            body.iter_mut().for_each(|s| each_expr_mut(s, f));
        }
        Stmt::Labeled { body, .. } => each_expr_mut(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => body
            .iter_mut()
            .chain(catch.iter_mut().flat_map(|c| &mut c.body))
            .chain(finally.iter_mut().flatten())
            .for_each(|s| each_expr_mut(s, f)),
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for case in cases {
                case.test.iter_mut().for_each(|e| f(e));
                case.body.iter_mut().for_each(|s| each_expr_mut(s, f));
            }
        }
        Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
    }
}

#[cfg(test)]
#[path = "field_forward_tests.rs"]
mod tests;
