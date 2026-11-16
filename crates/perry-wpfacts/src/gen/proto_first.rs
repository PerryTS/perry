//! Rule 3 (RAYBOX2): prototype data stored before any instance exists.
//!
//! `C.prototype.K = v` (or `this.prototype.K = v`) as a statement of a
//! static block of class `C`, `v` a literal: ECMA-262 15.7.14
//! ClassDefinitionEvaluation runs the static elements (step 31) before the
//! outer binding of `C` is initialized (BindingClassDeclarationEvaluation),
//! and the inner binding is reachable only from the class body, whose code
//! runs only through those elements. So when every static element before
//! the statement is itself such a store (no field initializer but a
//! literal, no computed key, no decorator), no code can have constructed a
//! `C` or a subclass (a subclass's heritage reads `C`: TDZ) before it: every
//! instance's chain finds `K` on `C.prototype` from its birth on. The solver
//! keeps the reads' `undefined` out only while the store is the key's one
//! writer on the prototype and nothing deletes, redefines or re-links the
//! chain (`Model::proto_first_holds`). A class nested in a function body is
//! not taken (its statics run per evaluation).

use super::*;
use perry_hir::{Expr, Function, Stmt};

/// The key of `C.prototype.K = lit` (`C` the class's name or `this`): a
/// named store, or the PutValue form whose key is a String literal and whose
/// receiver is the target (10.1.9 [[Set]] with Receiver the object).
fn proto_store<'e>(s: &'e Stmt, class: &str) -> Option<&'e str> {
    let Stmt::Expr(e) = s else {
        return None;
    };
    let (object, key, value) = match e {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } => (object.as_ref(), property.as_str(), value.as_ref()),
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => match key.as_ref() {
            Expr::String(k) if prototype_of(receiver, class) => (target.as_ref(), k.as_str(), value.as_ref()),
            _ => return None,
        },
        _ => return None,
    };
    (prototype_of(object, class) && literal(value)).then_some(key)
}

/// `C.prototype` (`this.prototype` in a static block of `C`).
fn prototype_of(e: &Expr, class: &str) -> bool {
    let Expr::PropertyGet {
        object: c,
        property: p,
        ..
    } = e
    else {
        return false;
    };
    p == "prototype"
        && match c.as_ref() {
            Expr::This => true,
            Expr::ClassRef(n) => n == class,
            _ => false,
        }
}

/// A literal: evaluating it runs no code.
fn literal(e: &Expr) -> bool {
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

/// The rule 3 statements of class `c`: (static block id, statement
/// index, key), in order, up to the first static element that may run code.
fn scan(c: &perry_hir::Class) -> Vec<(u32, usize, &str)> {
    let mut out = Vec::new();
    if c.is_nested
        || !c.decorators.is_empty()
        || c.static_fields.iter().any(|sf| {
            sf.key_expr.is_some()
                || !sf.decorators.is_empty()
                || sf.init.as_ref().is_some_and(|e| !literal(e))
        })
    {
        return out;
    }
    for b in c
        .static_methods
        .iter()
        .filter(|m| m.name.starts_with("__perry_static_init_"))
    {
        for (i, s) in b.body.iter().enumerate() {
            let Some(k) = proto_store(s, &c.name) else {
                return out;
            };
            out.push((b.id, i, k));
        }
    }
    out
}

impl<'a> Gen<'a> {
    /// Before any body is walked: the (class, key) pairs of rule 3.
    pub(crate) fn proto_first_class(&mut self, tm: TmplId, c: &perry_hir::Class) {
        for (_, _, k) in scan(c) {
            let a = self.m.atom(k);
            self.m.proto_first_keys.insert((tm, a));
        }
    }

    /// Walking static block `f`: its rule 3 statements, by address (the
    /// walked body may be a copy of the class's own).
    pub(crate) fn proto_first_stmts(&mut self, mi: u32, f: &'a Function) {
        if !f.name.starts_with("__perry_static_init_") {
            return;
        }
        let module = self.modules[mi as usize].1;
        let Some(c) = module
            .classes
            .iter()
            .find(|c| c.static_methods.iter().any(|m| m.id == f.id))
        else {
            return;
        };
        let Some(&tm) = self.mods[mi as usize].classes.get(&c.name) else {
            return;
        };
        for (b, i, _) in scan(c) {
            if let (true, Some(s)) = (b == f.id, f.body.get(i)) {
                self.m.proto_first_stmts.insert(s as *const Stmt as usize, tm);
            }
        }
    }
}
