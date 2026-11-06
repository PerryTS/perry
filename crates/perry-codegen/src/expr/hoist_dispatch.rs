//! The dispatch half of the `this.F` loop hoist's method inlining
//! (`perry-hir` `property_array_hoist::hoist_this_array_fields`).
//!
//! The hoist inlines `this.m(...)` only in a copy guarded by
//! `this.m === C.prototype.m`. That identity catches an override on a
//! subclass or on the instance, but not a REPLACED prototype method: after
//! `C.prototype.m = g` both sides read `g`. Codegen's own devirtualisation of
//! `this.m()` rests on the module-wide dispatch facts
//! (`ModuleDispatchFacts::prototype_is_stable`), and so does this guard: it
//! lowers to the runtime identity test only for a class whose prototype the
//! module provably never rewrites, and to a constant mismatch otherwise. A
//! module the table has no entry for (another thread's, a test) gets the
//! constant: the original loop runs.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

fn table() -> &'static Mutex<HashMap<String, HashSet<String>>> {
    static TABLE: OnceLock<Mutex<HashMap<String, HashSet<String>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the classes of module `slug` whose prototypes are stable.
pub(crate) fn publish(slug: String, stable: HashSet<String>) {
    if let Ok(mut t) = table().lock() {
        t.insert(slug, stable);
    }
}

pub(crate) fn prototype_is_stable(slug: &str, class: &str) -> bool {
    table()
        .lock()
        .ok()
        .and_then(|t| t.get(slug).map(|s| s.contains(class)))
        .unwrap_or(false)
}

/// `(class, method)` when `left === right` is the hoist's dispatch check
/// `this.m === C.prototype.m` (either operand order).
pub(crate) fn dispatch_check<'e>(
    left: &'e perry_hir::Expr,
    right: &'e perry_hir::Expr,
) -> Option<(&'e str, &'e str)> {
    use perry_hir::Expr;
    let side = |this_side: &'e Expr, proto_side: &'e Expr| -> Option<(&'e str, &'e str)> {
        let Expr::PropertyGet {
            object: this_obj,
            property: m1,
            ..
        } = this_side
        else {
            return None;
        };
        let Expr::PropertyGet {
            object: proto,
            property: m2,
            ..
        } = proto_side
        else {
            return None;
        };
        let Expr::PropertyGet {
            object: class_ref,
            property: p,
            ..
        } = proto.as_ref()
        else {
            return None;
        };
        let Expr::ClassRef(class) = class_ref.as_ref() else {
            return None;
        };
        (matches!(this_obj.as_ref(), Expr::This) && m1 == m2 && p == "prototype")
            .then_some((class.as_str(), m1.as_str()))
    };
    side(left, right).or_else(|| side(right, left))
}
