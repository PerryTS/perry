//! Which class TEMPLATE a class-valued expression names, for passes that
//! reason about a factory's returned class statically.

use super::expr::Expr;

/// The class template `expr` evaluates to, when that is statically known.
///
/// Recognizes a bare `ClassRef`, the trailing element of a `Sequence` (the
/// lowering prefixes a class value with its side-effect registrations), and a
/// *bare* `ClassExprFresh` — one with no per-evaluation statics, computed keys,
/// captures, or self-binding. The bare form is what a function-body class
/// expression with dynamic heritage (`return class extends Base {}`) lowers to
/// since #11042: it needs a distinct class object per evaluation only because
/// its parent can differ between evaluations, and its template is still the
/// class every evaluation instantiates. A `ClassExprFresh` that carries its
/// own per-evaluation state is deliberately not recognized — static passes
/// that clone or alias the template would drop that state.
pub fn class_value_template_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::ClassRef(name) => Some(name.as_str()),
        Expr::Sequence(parts) => parts.last().and_then(class_value_template_name),
        Expr::ClassExprFresh {
            template,
            evaluation_owner: None,
            named_statics,
            computed_keys,
            computed_statics,
            static_init_order,
            captured_args,
        } if named_statics.is_empty()
            && computed_keys.is_empty()
            && computed_statics.is_empty()
            && static_init_order.is_empty()
            && captured_args.is_empty() =>
        {
            Some(template.as_str())
        }
        _ => None,
    }
}
