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
//! that name: then it is the realm's own data property), and a
//! construction `new Y( args )` of a plain class (R9READ5,
//! `inert_construct`). A class
//! declaration's own evaluation statements (parent registration, computed
//! names, static elements, decorators) start at its `decl_stmt` and are
//! scanned like any other. Anything else ends every open window.

use perry_hir::{Class, CompareOp, Expr, Module, Stmt};

/// Per settled store: (class name, key, the global names read between the
/// class declaration and the store, `X.prototype.k` (true) or the static
/// `X.k` (false)). R9BIRTH7: a settled `X.k = v` is the class object's own
/// data property from the store on, as a class-body static field is from
/// the declaration on: no code can read `X` between them (the same window).
pub(crate) fn settled_proto_stores(module: &Module) -> Vec<(String, String, Win, bool)> {
    let mut out = Vec::new();
    // Open windows: class name -> what the window read since its declaration.
    let mut open: Vec<(String, Win)> = Vec::new();
    let n = module.init.len();
    for i in 0..=n {
        for c in &module.classes {
            if c.decl_stmt == Some(i as u32) && !c.is_nested {
                open.push((c.name.clone(), Win::default()));
            }
        }
        if i == n {
            break;
        }
        let mut globals = Win::default();
        let inert = match &module.init[i] {
            Stmt::Let { init, .. } => init
                .as_ref()
                .is_none_or(|e| inert_value(module, e, i, &mut globals)),
            // A class method under a known Symbol key (15.7.14 step 27:
            // ClassElementEvaluation): ToPropertyKey of a Symbol is the
            // Symbol (7.1.19), and DefineMethodProperty (10.2.8) defines a
            // data property: no code runs.
            Stmt::Expr(Expr::RegisterClassComputedMethod { key_expr, .. }) => {
                known_symbol_key(key_expr)
            }
            Stmt::Expr(Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            }) => match (target.as_ref(), key.as_ref()) {
                (t, Expr::String(k)) if same_target(t, receiver) => {
                    match store_target(module, t, k) {
                        Some((x, proto)) if inert_value(module, value, i, &mut globals) => {
                            for (name, g) in open.iter_mut() {
                                g.extend(&globals);
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
                g.extend(&globals);
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
            Expr::PropertyGet {
                object: a,
                property: pa,
                ..
            },
            Expr::PropertyGet {
                object: b,
                property: pb,
                ..
            },
        ) => pa == pb && same_target(a, b),
        _ => false,
    }
}

/// `X.prototype` (true) or `X` (false) for a class `X` of this module whose
/// chain is in this module and declares no accessor of `k`, with `k` no key
/// whose [[Set]] runs code or fails on a class.
fn store_target(module: &Module, t: &Expr, k: &str) -> Option<(String, bool)> {
    if matches!(
        k,
        "__proto__" | "prototype" | "name" | "length" | "caller" | "arguments"
    ) {
        return None;
    }
    let (x, proto) = match t {
        Expr::ClassRef(x) => (x, false),
        Expr::PropertyGet {
            object, property, ..
        } if property == "prototype" => match object.as_ref() {
            Expr::ClassRef(x) => (x, true),
            _ => return None,
        },
        _ => return None,
    };
    let mut c: &Class = module
        .classes
        .iter()
        .find(|c| &c.name == x && !c.is_nested)?;
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
        Expr::SymbolFor(_) | Expr::WellKnownSymbol(_) => e.known_symbol().is_some(),
        Expr::Call { callee, args, .. } => {
            matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == "js_to_property_key")
                && matches!(args.as_slice(), [a] if known_symbol_key(a))
        }
        _ => false,
    }
}

fn inert_value(module: &Module, e: &Expr, at: usize, globals: &mut Win) -> bool {
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
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::GlobalGet(0)) => {
            globals.globals.push(property.clone());
            true
        }
        // R9READ3: an array literal without spread or hole: ArrayCreate and
        // one CreateDataPropertyOrThrow per element (13.2.4.1
        // ArrayAccumulation, 10.4.2.2) on the new array run no program code
        // when no element evaluation runs any.
        Expr::Array(elems, _) => elems.iter().all(|a| inert_value(module, a, at, globals)),
        // R9READ3: `Y.prototype.m` for an instance method `m` of the class
        // body of `Y` (a class of this module) with no accessor of `m` there.
        // `Y.prototype` is an own non-writable data property of `Y`
        // (15.7.14 MakeConstructor), and ClassDefinitionEvaluation defines
        // `m` on it as an own data property (10.2.8 DefineMethodProperty):
        // OrdinaryGet (10.1.8.1) finds an own data property at both steps
        // and runs no code. An inert store in the window (`store_target`:
        // no accessor of the key on the chain) keeps `m` a data property;
        // every other statement closes the window.
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::PropertyGet {
                object: y,
                property: pp,
                ..
            } if pp == "prototype" => match y.as_ref() {
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
        Expr::New {
            class_name, args, ..
        } => {
            (module.classes.iter().any(|c| {
                &c.name == class_name
                    && c.name.starts_with("__AnonShape_")
                    && c.methods.is_empty()
                    && c.getters.is_empty()
                    && c.setters.is_empty()
                    && c.extends.is_none()
                    && c.extends_name.is_none()
            }) || inert_construct(module, class_name, at, globals))
                && args.iter().all(|a| inert_value(module, a, at, globals))
        }
        _ => false,
    }
}

/// What a window read on the way: global names (`derive` checks each is
/// the realm's own data property), keys an inert construction stored
/// through `[[Set]]` and the classes it constructed (`derive` checks that
/// no accessor may be defined under any of the keys (`needs_accessor_walk`)
/// and that no HARD operation or unknown code reaches the classes'
/// prototypes).
#[derive(Clone, Debug, Default)]
pub(crate) struct Win {
    pub globals: Vec<String>,
    pub keys: Vec<String>,
    pub classes: Vec<String>,
}

impl Win {
    fn extend(&mut self, o: &Win) {
        self.globals.extend(o.globals.iter().cloned());
        self.keys.extend(o.keys.iter().cloned());
        self.classes.extend(o.classes.iter().cloned());
    }
}

/// R9READ5: `new Y( args )` runs no program code beyond Y's constructor
/// body when Y is a class of this module evaluated before statement `at`
/// (`decl_stmt` <= `at`; else its binding is in its TDZ: a ReferenceError,
/// 9.1.1.1.6) with no
/// heritage (no super constructor, 10.2.2 step 6), whose computed member
/// names are not evaluated, whose instance fields have no computed key and
/// a primitive literal or no initializer (InitializeInstanceElements
/// defines them: CreateDataPropertyOrThrow, no setter), whose parameters
/// are plain with a primitive literal or no default, and whose body is
/// only stores `this.k = v` and `Y.prototype.k = v` of a parameter or a
/// primitive literal. [[Construct]] (10.2.2) reads `Y.prototype` in
/// OrdinaryCreateFromConstructor (10.1.13): an own non-writable data
/// property of the class (15.7.14), no code. Each store is OrdinarySet
/// (10.1.9.2): it runs code only through an accessor under `k` on the
/// chain (Y.prototype, Object.prototype) and fails only on a non-writable
/// `k` there: `k` is no accessor of Y's body, no member of
/// `Object.prototype` and not `__proto__`; `derive` checks the rest (the
/// keys and classes noted in `w`). CLOSED-WORLD PREMISE (owner
/// 2026-11-14): every accessor definition is in the program.
fn inert_construct(module: &Module, y: &str, at: usize, w: &mut Win) -> bool {
    let Some(c) = module.classes.iter().find(|c| c.name == y && !c.is_nested) else {
        return false;
    };
    if !c.decl_stmt.is_some_and(|d| (d as usize) <= at)
        || c.extends.is_some()
        || c.extends_name.is_some()
        || c.extends_expr.is_some()
        || c.native_extends.is_some()
        || c.computed_members.iter().any(|m| m.is_evaluated())
        || !c
            .fields
            .iter()
            .all(|f| f.key_expr.is_none() && f.init.as_ref().is_none_or(prim_literal))
    {
        return false;
    }
    let mut keys = Vec::new();
    if let Some(ctor) = &c.constructor {
        if !ctor.params.iter().all(|p| {
            !p.is_rest && p.decorators.is_empty() && p.default.as_ref().is_none_or(prim_literal)
        }) {
            return false;
        }
        let param = |e: &Expr| {
            prim_literal(e)
                || matches!(e, Expr::LocalGet(id) if ctor.params.iter().any(|p| p.id == *id))
        };
        let settable = |k: &str| {
            k != "__proto__"
                && !crate::builtins::members::object_proto_has(k)
                && !c.getters.iter().any(|(n, _)| n == k)
                && !c.setters.iter().any(|(n, _)| n == k)
        };
        let is_param = |id: &u32| ctor.params.iter().any(|p| p.id == *id);
        for st in &ctor.body {
            let ok = match st {
                // The lowered parameter default `if (p === undefined) p = lit`:
                // a comparison with `undefined` runs no code (7.2.14 / 7.2.15:
                // no ToPrimitive against Undefined).
                Stmt::If {
                    condition:
                        Expr::Compare {
                            op: CompareOp::Eq,
                            left,
                            right,
                        },
                    then_branch,
                    else_branch: None,
                } => {
                    matches!((left.as_ref(), right.as_ref()), (Expr::LocalGet(id), Expr::Undefined) if is_param(id))
                        && matches!(then_branch.as_slice(), [Stmt::Expr(Expr::LocalSet(id, v))] if is_param(id) && prim_literal(v))
                }
                Stmt::Expr(Expr::PropertySet {
                    object,
                    property,
                    value,
                    ..
                }) => {
                    matches!(object.as_ref(), Expr::This) && settable(property) && param(value) && {
                        keys.push(property.clone());
                        true
                    }
                }
                Stmt::Expr(Expr::PutValueSet {
                    target,
                    key,
                    value,
                    receiver,
                    ..
                }) => match (target.as_ref(), key.as_ref()) {
                    (Expr::This, Expr::String(k)) => {
                        matches!(receiver.as_ref(), Expr::This) && settable(k) && param(value) && {
                            keys.push(k.clone());
                            true
                        }
                    }
                    (t, Expr::String(k)) => {
                        same_target(t, receiver)
                            && store_target(module, t, k).is_some_and(|(x, proto)| proto && x == y)
                            && !crate::builtins::members::object_proto_has(k)
                            && param(value)
                            && {
                                keys.push(k.clone());
                                true
                            }
                    }
                    _ => false,
                },
                _ => false,
            };
            if !ok {
                return false;
            }
        }
    }
    w.keys.extend(keys);
    w.classes.push(y.to_string());
    true
}

fn prim_literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
    )
}
