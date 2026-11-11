//! What running a function can do to anything but its own receiver, decided
//! from the function alone.
//!
//! A call is opaque to the passes around it: it may run any code, so every
//! fact about every object dies at it. A function is **closed** when its body
//! proves the opposite: running it executes no code outside the body and
//! writes nothing but fields of its own receiver. Then a caller's facts about
//! other objects survive the call, and each parameter has one of two fates:
//! it is dropped, or the receiver keeps it. Those fates are the general
//! parameter-fate rule (`effects_fate.rs`, [`Fates`]) applied to the
//! constructor body: one definition of "kept" for every consumer.
//!
//! The first user is construction. `new C(...)` is a call, so the inliner
//! forgot the class of every earlier `new` at it: `const a = new V(); const b
//! = new V(); a.dot(b)` could not inline `dot`, and both objects stayed on
//! the heap. With a closed construction, `a` is still known at the call.
//!
//! The rule accepts only what it can read off the body, and everything else
//! is open (the answer `None`):
//!
//! * a value is a parameter or a literal. No operator: `x + 1` may run a
//!   `valueOf`, and a declared type is not a proof of what arrives;
//! * a statement stores a value into a named field of `this`, or returns
//!   nothing;
//! * the class and its field initializers declare no accessor for a stored
//!   name, no computed member and no decorator, and the class extends nothing:
//!   the store cannot reach a setter the class chain defines;
//! * the function is an ordinary one: no defaults, rest, `arguments`,
//!   captures, `async` or generator.
//!
//! Two classes of one name in a module leave both open, because `new` names
//! its class by name.

use std::cell::RefCell;
use std::collections::HashMap;

use perry_hir::{Class, Expr, Module, Stmt};

#[path = "effects_fate.rs"]
mod fate;
pub use fate::{
    body_fates, direct_target, BodyOnly, Fate, FateProof, Fates, FnFates, ParamFate, Proof,
};

/// A closed function: see the module comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    /// One entry per declared parameter: `Dropped` or `KeptByReceiver`,
    /// the general rule's answer (`effects_fate`) for this body.
    pub kept: Vec<Fate>,
}

/// The closed constructions of one module, by class name.
#[derive(Default)]
pub struct Effects {
    construction: HashMap<String, Closed>,
}

impl Effects {
    pub fn of_module(module: &Module) -> Self {
        let mut construction = HashMap::new();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for class in &module.classes {
            *seen.entry(class.name.as_str()).or_insert(0) += 1;
        }
        for class in &module.classes {
            if seen[class.name.as_str()] != 1 {
                continue;
            }
            if let Some(closed) = closed_construction(class) {
                construction.insert(class.name.clone(), closed);
            }
        }
        Self { construction }
    }

    /// `new <class>(...)`: field initializers, then the constructor body.
    pub fn construction(&self, class: &str) -> Option<&Closed> {
        self.construction.get(class)
    }
}

fn is_value(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::LocalGet(_)
            | Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
    )
}

/// `this.<name> = <value>` as `(name, value)`.
fn own_field_store(expr: &Expr) -> Option<(&str, &Expr)> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if matches!(object.as_ref(), Expr::This) => Some((property, value)),
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            match key.as_ref() {
                Expr::String(name) => Some((name, value)),
                _ => None,
            }
        }
        _ => None,
    }
}

use fate::ordinary;

fn closed_construction(class: &Class) -> Option<Closed> {
    if class.extends.is_some()
        || class.extends_name.is_some()
        || class.native_extends.is_some()
        || class.extends_expr.is_some()
        || !class.decorators.is_empty()
        || !class.computed_members.is_empty()
    {
        return None;
    }
    for field in &class.fields {
        if field.key_expr.is_some()
            || !field.decorators.is_empty()
            || has_accessor(class, &field.name)
            || field
                .init
                .as_ref()
                .is_some_and(|e| !is_value_without_locals(e))
        {
            return None;
        }
    }
    let Some(ctor) = &class.constructor else {
        return Some(Closed { kept: Vec::new() });
    };
    if !ordinary(ctor) {
        return None;
    }
    for stmt in &ctor.body {
        match stmt {
            Stmt::Return(None) => {}
            Stmt::Expr(e) => {
                let (name, value) = own_field_store(e)?;
                if has_accessor(class, name) || !is_value(value) {
                    return None;
                }
                // A local that is not a parameter cannot exist: the body
                // declares none.
                if let Expr::LocalGet(id) = value {
                    ctor.params.iter().position(|p| p.id == *id)?;
                }
            }
            _ => return None,
        }
    }
    // The fates are the general rule's: the checks above prove every store
    // writes an own data field of the receiver and reaches no setter.
    let fates = body_fates(ctor, &OwnStores, &Fates::default());
    let kept = fates.params.iter().map(|p| p.fate).collect();
    Some(Closed { kept })
}

/// The proof a closed construction gives the general rule: every store into
/// `this` is an own data store (checked by `closed_construction` first).
struct OwnStores;

impl FateProof for OwnStores {
    fn read(&self, _: perry_hir::types::FuncId, _: &Expr, _: &str, _: u32) -> Option<Proof> {
        None
    }
    fn store(&self, _: perry_hir::types::FuncId, _: &str) -> Option<Proof> {
        Some(Proof::Proven)
    }
    fn targets(&self, callee: &Expr) -> Option<(Vec<perry_hir::types::FuncId>, Proof)> {
        direct_target(callee)
    }
}

/// Does `class` itself declare a getter or setter named `name`?
pub(crate) fn has_accessor(class: &Class, name: &str) -> bool {
    class
        .getters
        .iter()
        .chain(&class.setters)
        .any(|(n, _)| n == name)
}

/// A field initializer runs outside the constructor's parameters.
fn is_value_without_locals(expr: &Expr) -> bool {
    is_value(expr) && !matches!(expr, Expr::LocalGet(_))
}

thread_local! {
    static ACTIVE: RefCell<Option<Effects>> = const { RefCell::new(None) };
}

/// Makes one module's effects answerable for the length of a pass whose
/// helpers have no path to hand them down (the inliner's receiver facts are
/// cleared from eighteen places). Without a session every question is open.
pub(crate) struct Session;

impl Session {
    pub(crate) fn start(module: &Module) -> Self {
        ACTIVE.with(|active| {
            let previous = active.borrow_mut().replace(Effects::of_module(module));
            assert!(previous.is_none(), "nested effects session");
        });
        Self
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}

/// Is `new <class>(...)` closed in the module of the running session?
pub(crate) fn construction_is_closed(class: &str) -> bool {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|effects| effects.construction(class).is_some())
    })
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
