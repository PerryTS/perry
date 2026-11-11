//! What running a function can do to anything but its own receiver, decided
//! from the function alone.
//!
//! A call is opaque to the passes around it: it may run any code, so every
//! fact about every object dies at it. A function is **closed** when its body
//! proves the opposite: running it executes no code outside the body and
//! writes nothing but fields of its own receiver. Then a caller's facts about
//! other objects survive the call, and each parameter has one of two fates:
//! it is dropped, or the receiver keeps it.
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

use perry_hir::{Class, Expr, Function, Module, Stmt};

/// The fate of one parameter of a closed function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// Not stored anywhere: it does not outlive the call through this call.
    No,
    /// Stored in a field of the receiver: it lives as long as the receiver.
    ByReceiver,
}

/// A closed function: see the module comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    /// One entry per declared parameter.
    pub kept: Vec<Kept>,
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

fn ordinary(f: &Function) -> bool {
    !f.is_async
        && !f.is_generator
        && f.captures.is_empty()
        && f.decorators.is_empty()
        && f.params.iter().all(|p| {
            p.default.is_none()
                && !p.is_rest
                && p.arguments_object.is_none()
                && p.decorators.is_empty()
        })
}

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
    let has_accessor = |name: &str| {
        class
            .getters
            .iter()
            .chain(&class.setters)
            .any(|(n, _)| n == name)
    };
    for field in &class.fields {
        if field.key_expr.is_some()
            || !field.decorators.is_empty()
            || has_accessor(&field.name)
            || field.init.as_ref().is_some_and(|e| !is_value_without_locals(e))
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
    let mut kept = vec![Kept::No; ctor.params.len()];
    for stmt in &ctor.body {
        match stmt {
            Stmt::Return(None) => {}
            Stmt::Expr(e) => {
                let (name, value) = own_field_store(e)?;
                if has_accessor(name) || !is_value(value) {
                    return None;
                }
                if let Expr::LocalGet(id) = value {
                    // A local that is not a parameter cannot exist: the body
                    // declares none.
                    let at = ctor.params.iter().position(|p| p.id == *id)?;
                    kept[at] = Kept::ByReceiver;
                }
            }
            _ => return None,
        }
    }
    Some(Closed { kept })
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
