//! #10906: the synthetic shape class an object-literal method's `this` is
//! most likely to be.
//!
//! HIR lowers a closed-shape literal `{ a: 1, run() { … this.a … } }` to
//! `new __AnonShape_<hash>(1, <closure>)`, and each method becomes a plain
//! dynamic-`this` closure stored in a field slot. Nothing ties the closure's
//! `this` to that class, so every `this.field` in the method took the generic
//! per-site inline cache while the same read through a local binding of the
//! literal took the class-field slot path.
//!
//! This names the class as a *candidate*, never a proof. `this` in the method
//! is whatever the call binds (`f.call(other)`, an extracted method called
//! bare, the object after a shape transition), so the only consumers are the
//! guarded class-field paths whose runtime class-id/shape check owns a full
//! fallback — the same evidence level a `Named` local type hint has.

use std::collections::HashMap;

use perry_hir::{Expr, Module};

use super::scalar_method_dispatch::{for_each_expr, for_each_expr_in_stmts};

const ANON_SHAPE_PREFIX: &str = "__AnonShape_";

/// Map each dynamic-`this` method closure `func_id` constructed directly into
/// a closed-shape literal to that literal's `__AnonShape_*` class.
///
/// A closure that appears under two different shape classes has no single
/// candidate and is left out.
pub(crate) fn literal_method_home_classes(hir: &Module) -> HashMap<u32, String> {
    let mut homes: HashMap<u32, Option<String>> = HashMap::new();
    let mut visit = |expr: &Expr| {
        let Expr::New {
            class_name, args, ..
        } = expr
        else {
            return;
        };
        if !class_name.starts_with(ANON_SHAPE_PREFIX) {
            return;
        }
        for arg in args {
            if let Expr::Closure {
                func_id,
                captures_this: false,
                enclosing_class: None,
                is_arrow: false,
                ..
            } = arg
            {
                homes
                    .entry(*func_id)
                    .and_modify(|home| {
                        if home.as_deref() != Some(class_name.as_str()) {
                            *home = None;
                        }
                    })
                    .or_insert_with(|| Some(class_name.clone()));
            }
        }
    };

    for_each_expr_in_stmts(&hir.init, &mut visit);
    for f in &hir.functions {
        for_each_expr_in_stmts(&f.body, &mut visit);
    }
    for c in &hir.classes {
        let bodies = c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.static_methods.iter())
            .chain(c.computed_members.iter().map(|m| &m.function));
        for f in bodies {
            for_each_expr_in_stmts(&f.body, &mut visit);
        }
        for init in c
            .fields
            .iter()
            .chain(c.static_fields.iter())
            .filter_map(|field| field.init.as_ref())
        {
            for_each_expr(init, &mut visit);
        }
    }
    for init in hir.globals.iter().filter_map(|g| g.init.as_ref()) {
        for_each_expr(init, &mut visit);
    }

    homes
        .into_iter()
        .filter_map(|(func_id, home)| home.map(|class| (func_id, class)))
        .collect()
}
