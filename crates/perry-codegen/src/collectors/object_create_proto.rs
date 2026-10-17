//! Step 5C: the prototype a value made by `Object.create(P)` most likely
//! inherits from, as a CANDIDATE class for its method sites.
//!
//! `const P = { m() { ... } }` lowers to `new __AnonShape_<hash>(...)`, and
//! the completed shapes of that class name `m`'s body in a ConstFn lane. A
//! receiver made by `Object.create(P)` inherits `m` from P, so its method
//! site's learned (inherited ConstFn) hit records that body. Naming P's class
//! as the receiver's candidate lets the site call the body directly and inline
//! it, as `this.m()` in P's own method already does through its home class.
//!
//! This is never a proof. The shape words are: the learned hit compares the
//! receiver's word (which pins the holder) and the holder's word (which pins
//! the body), and only then compares the recorded body with the candidate. A
//! wrong candidate (the prototype's method replaced, another prototype set, an
//! own `m`) records another body or misses, and takes the generic path.
//!
//! Sources, followed to a fixed point:
//! * `Object.create(P)` where `P` is a `const` binding whose initializer is a
//!   closed-shape literal;
//! * a `const` binding initialized by a candidate value (a local, a captured
//!   local and a module-level `const` are all one binding id);
//! * a call of a module function (not async, not a generator) whose every
//!   `return` is a candidate value of the same class.

use std::collections::HashMap;

use perry_hir::{Expr, Module, Stmt};

const ANON_SHAPE_PREFIX: &str = "__AnonShape_";

/// Candidate prototype classes of `Object.create` values, by binding id and by
/// factory function id, and the literal class of each prototype binding.
#[derive(Debug, Default, Clone)]
pub(crate) struct ObjectCreateProtos {
    pub protos: HashMap<u32, String>,
    pub values: HashMap<u32, String>,
    pub functions: HashMap<u32, String>,
}

impl ObjectCreateProtos {
    /// The candidate prototype class of the value `e` evaluates to.
    pub(crate) fn of(&self, e: &Expr) -> Option<&String> {
        match source(e)? {
            Source::Create(p) => self.protos.get(&p),
            Source::Call(f) => self.functions.get(&f),
            Source::Local(id) => self.values.get(&id),
            Source::Literal(_) => None,
        }
    }
}

enum Source {
    Create(u32),
    Call(u32),
    Local(u32),
    Literal(String),
}

fn source(e: &Expr) -> Option<Source> {
    match e {
        Expr::ObjectCreate(proto, _) => match proto.as_ref() {
            Expr::LocalGet(id) => Some(Source::Create(*id)),
            _ => None,
        },
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::FuncRef(f) => Some(Source::Call(*f)),
            _ => None,
        },
        Expr::LocalGet(id) => Some(Source::Local(*id)),
        Expr::New { class_name, .. } if class_name.starts_with(ANON_SHAPE_PREFIX) => {
            Some(Source::Literal(class_name.clone()))
        }
        _ => None,
    }
}

pub(crate) fn object_create_protos(hir: &Module) -> ObjectCreateProtos {
    // Every `const` binding with an initializer. A binding id bound twice has
    // no single source.
    let mut bindings: HashMap<u32, Option<Source>> = HashMap::new();
    let mut bind = |stmt: &Stmt| {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        {
            bindings
                .entry(*id)
                .and_modify(|b| *b = None)
                .or_insert_with(|| source(init));
        }
    };
    let mut bodies: Vec<&[Stmt]> = vec![&hir.init];
    bodies.extend(hir.functions.iter().map(|f| f.body.as_slice()));
    for c in &hir.classes {
        bodies.extend(
            c.constructor
                .iter()
                .chain(&c.methods)
                .chain(&c.static_methods)
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
                .map(|f| f.body.as_slice()),
        );
    }
    for body in &bodies {
        super::ptr_shape_elements::walk_stmts(body, &mut bind);
        super::scalar_method_dispatch::for_each_expr_in_stmts(body, &mut |e| {
            if let Expr::Closure { body, .. } = e {
                super::ptr_shape_elements::walk_stmts(body, &mut bind);
            }
        });
    }

    // Each factory: the sources of all its returns.
    let mut factories: Vec<(u32, Vec<Source>)> = Vec::new();
    for f in &hir.functions {
        if f.is_async || f.is_generator {
            continue;
        }
        let mut returns = Vec::new();
        let mut ok = true;
        super::ptr_shape_elements::walk_stmts(&f.body, &mut |s| match s {
            Stmt::Return(Some(e)) => match source(e) {
                Some(src) => returns.push(src),
                None => ok = false,
            },
            Stmt::Return(None) => ok = false,
            _ => {}
        });
        if ok && !returns.is_empty() {
            factories.push((f.id, returns));
        }
    }

    let mut out = ObjectCreateProtos::default();
    for (id, b) in &bindings {
        if let Some(Source::Literal(class)) = b {
            out.protos.insert(*id, class.clone());
        }
    }
    let resolve = |src: &Source, out: &ObjectCreateProtos| -> Option<String> {
        match src {
            Source::Create(p) => out.protos.get(p).cloned(),
            Source::Call(f) => out.functions.get(f).cloned(),
            Source::Local(id) => out.values.get(id).cloned(),
            Source::Literal(_) => None,
        }
    };
    loop {
        let mut changed = false;
        for (id, b) in &bindings {
            if out.values.contains_key(id) {
                continue;
            }
            if let Some(class) = b.as_ref().and_then(|src| resolve(src, &out)) {
                out.values.insert(*id, class);
                changed = true;
            }
        }
        for (fid, returns) in &factories {
            if out.functions.contains_key(fid) {
                continue;
            }
            let first = resolve(&returns[0], &out);
            if let Some(class) = first.filter(|c| {
                returns[1..]
                    .iter()
                    .all(|r| resolve(r, &out).as_ref() == Some(c))
            }) {
                out.functions.insert(*fid, class);
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}
