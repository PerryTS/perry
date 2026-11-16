//! R9BIRTH6: the top-level prototype stores that complete before any code
//! can read through the prototype.
//!
//! ECMA-262 16.2.1.5.3 InnerModuleEvaluation runs a module's top-level
//! statements in order; a class declaration is evaluated at its statement
//! (15.7.14 ClassDefinitionEvaluation) and its binding is in its TDZ until
//! then (14.3.1, 9.1.1.1.6 GetBindingValue): every earlier reference, from
//! this module, a hoisted function or a cyclic importer, throws. So no
//! object whose chain holds `X.prototype` (nor `X.prototype` itself) can be
//! reached before the declaration. When the statements from the
//! declaration up to a store `X.prototype.k = v` run no code (each is
//! *inert*: no call, construction, accessor or unknown operation can run
//! between them), the store has completed before any read or call through
//! `X.prototype` can execute: `k` is then present at every such read
//! (until a `delete` or `defineProperty`, which `derive` refuses).
//!
//! Inert statements: `let`/`const` of an inert value, a named store
//! `Y.prototype.k = v` / `Y.k = v` into a class of this module whose
//! chain (all in this module) declares no accessor of `k` (no setter can
//! run, 10.1.9.2) with `k` none of `__proto__` (an Object.prototype
//! accessor), `prototype`, `name`, `length` (own non-writable data of a
//! class) or `caller` / `arguments` (Function.prototype accessors).
//! Inert values: literals, a binding or class reference, a closure (its
//! creation runs no code), an object literal or an array literal (no
//! spread, no hole) of inert values, a read `Y.prototype.m` of a method of
//! the class body of `Y` (R9READ3), and a read of
//! a global builtin name (`derive` checks the program made no global of
//! that name: then it is the realm's own data property). A class
//! declaration's own evaluation statements (parent registration, computed
//! names, static elements, decorators) start at its `decl_stmt` and are
//! scanned like any other. Anything else ends every open window.

use perry_hir::{Class, Expr, Module, Stmt};

/// Per settled store: (class name, key, the global names read between the
/// class declaration and the store, `X.prototype.k` (true) or the static
/// `X.k` (false)). R9BIRTH7: a settled `X.k = v` is the class object's own
/// data property from the store on, as a class-body static field is from
/// the declaration on: no code can read `X` between them (the same window).
pub(crate) fn settled_proto_stores(module: &Module) -> Vec<(String, String, Vec<String>, bool)> {
    let mut out = Vec::new();
    // Open windows: class name -> globals read since its declaration.
    let mut open: Vec<(String, Vec<String>)> = Vec::new();
    let n = module.init.len();
    for i in 0..=n {
        for c in &module.classes {
            if c.decl_stmt == Some(i as u32) && !c.is_nested {
                open.push((c.name.clone(), Vec::new()));
            }
        }
        if i == n {
            break;
        }
        let mut globals = Vec::new();
        let inert = match &module.init[i] {
            Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| inert_value(module, e, &mut globals)),
            // A class method under a known Symbol key (15.7.14 step 27:
            // ClassElementEvaluation): ToPropertyKey of a Symbol is the
            // Symbol (7.1.19), and DefineMethodProperty (10.2.8) defines a
            // data property: no code runs.
            Stmt::Expr(Expr::RegisterClassComputedMethod { key_expr, .. }) => known_symbol_key(key_expr),
            Stmt::Expr(Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            }) => match (target.as_ref(), key.as_ref()) {
                (t, Expr::String(k)) if same_target(t, receiver) => {
                    match store_target(module, t, k) {
                        Some((x, proto)) if inert_value(module, value, &mut globals) => {
                            for (name, g) in open.iter_mut() {
                                g.extend(globals.iter().cloned());
                                if *name == x {
                                    out.push((x.clone(), k.clone(), g.clone(), proto));
                                }
                            }
                            continue;
                        }
                        _ => false,
                    }
                }
                _ => false,
            },
            _ => false,
        };
        if inert {
            for (_, g) in open.iter_mut() {
                g.extend(globals.iter().cloned());
            }
        } else {
            open.clear();
        }
    }
    out
}

fn same_target(t: &Expr, r: &Expr) -> bool {
    match (t, r) {
        (Expr::ClassRef(a), Expr::ClassRef(b)) => a == b,
        (
            Expr::PropertyGet { object: a, property: pa, .. },
            Expr::PropertyGet { object: b, property: pb, .. },
        ) => pa == pb && same_target(a, b),
        _ => false,
    }
}

/// `X.prototype` (true) or `X` (false) for a class `X` of this module whose
/// chain is in this module and declares no accessor of `k`, with `k` no key
/// whose [[Set]] runs code or fails on a class.
fn store_target(module: &Module, t: &Expr, k: &str) -> Option<(String, bool)> {
    if matches!(k, "__proto__" | "prototype" | "name" | "length" | "caller" | "arguments") {
        return None;
    }
    let (x, proto) = match t {
        Expr::ClassRef(x) => (x, false),
        Expr::PropertyGet { object, property, .. } if property == "prototype" => match object.as_ref() {
            Expr::ClassRef(x) => (x, true),
            _ => return None,
        },
        _ => return None,
    };
    let mut c: &Class = module.classes.iter().find(|c| &c.name == x && !c.is_nested)?;
    loop {
        if c.getters.iter().any(|(n, _)| n == k) || c.setters.iter().any(|(n, _)| n == k) {
            return None;
        }
        // A computed member whose key is a known Symbol (`known_symbol`: a
        // well-known symbol or a registered `Symbol.for("lit")`, resolved
        // at compile time) is named by that Symbol, never by the String `k`
        // (7.1.19 ToPropertyKey), and its key evaluation runs no program
        // code: it declares no accessor of `k`. An evaluated key may be `k`.
        if c.extends_expr.is_some()
            || c.native_extends.is_some()
            || c.computed_members.iter().any(|m| m.is_evaluated())
        {
            return None;
        }
        match c.extends {
            None if c.extends_name.is_none() => break,
            Some(id) => c = module.classes.iter().find(|p| p.id == id)?,
            None => return None,
        }
    }
    Some((x.clone(), proto))
}

/// A computed key that the lowering resolved to a known Symbol
/// (`ClassComputedMember::known_symbol`), possibly under its
/// `js_to_property_key` conversion.
fn known_symbol_key(e: &Expr) -> bool {
    match e {
        Expr::SymbolFor(inner) => matches!(inner.as_ref(), Expr::String(_)),
        Expr::Call { callee, args, .. } => {
            matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == "js_to_property_key")
                && matches!(args.as_slice(), [a] if known_symbol_key(a))
        }
        _ => false,
    }
}

fn inert_value(module: &Module, e: &Expr, globals: &mut Vec<String>) -> bool {
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::ClassRef(_)
        | Expr::LocalGet(_)
        | Expr::Closure { .. } => true,
        Expr::PropertyGet { object, property, .. } if matches!(object.as_ref(), Expr::GlobalGet(0)) => {
            globals.push(property.clone());
            true
        }
        // R9READ3: an array literal without spread or hole: ArrayCreate and
        // one CreateDataPropertyOrThrow per element (13.2.4.1
        // ArrayAccumulation, 10.4.2.2) on the new array run no program code
        // when no element evaluation runs any.
        Expr::Array(elems, _) => elems.iter().all(|a| inert_value(module, a, globals)),
        // R9READ3: `Y.prototype.m` for an instance method `m` of the class
        // body of `Y` (a class of this module) with no accessor of `m` there.
        // `Y.prototype` is an own non-writable data property of `Y`
        // (15.7.14 MakeConstructor), and ClassDefinitionEvaluation defines
        // `m` on it as an own data property (10.2.8 DefineMethodProperty):
        // OrdinaryGet (10.1.8.1) finds an own data property at both steps
        // and runs no code. An inert store in the window (`store_target`:
        // no accessor of the key on the chain) keeps `m` a data property;
        // every other statement closes the window.
        Expr::PropertyGet { object, property, .. } => match object.as_ref() {
            Expr::PropertyGet { object: y, property: pp, .. } if pp == "prototype" => match y.as_ref() {
                Expr::ClassRef(x) => module.classes.iter().any(|c| {
                    &c.name == x
                        && !c.is_nested
                        && property != "__proto__"
                        && c.methods.iter().any(|m| &m.name == property)
                        && !c.getters.iter().any(|(n, _)| n == property)
                        && !c.setters.iter().any(|(n, _)| n == property)
                }),
                _ => false,
            },
            _ => false,
        },
        // An object literal's layout class: its construction stores the
        // values as own data keys (13.2.5.5), no constructor code of the
        // program runs.
        Expr::New { class_name, args, .. } => {
            module.classes.iter().any(|c| {
                &c.name == class_name && c.name.starts_with("__AnonShape_") && c.methods.is_empty()
                    && c.getters.is_empty() && c.setters.is_empty() && c.extends.is_none()
                    && c.extends_name.is_none()
            }) && args.iter().all(|a| inert_value(module, a, globals))
        }
        _ => false,
    }
}
