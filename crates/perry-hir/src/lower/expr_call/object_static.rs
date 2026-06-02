//! `Object.<staticMethod>` alias-call HIR builder.
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the only consumer is `lower_call_inner` inside this module.

use crate::ir::*;

/// Issue #886: synthesize the dedicated HIR variant for an indirect call
/// through an `Object.<staticMethod>` alias. Mirrors the literal-callee
/// recogniser in `lower_call` (the `obj_name == "Object"` arm) but skips
/// the AST-shape sub-cases that depend on argument-AST inspection
/// (`Object.assign({}, …)` fresh-target ObjectSpread fold,
/// `Object.defineProperties` static-descriptor sequence fold). The aliased
/// shape can't benefit from those AST-time folds anyway — the literal
/// recogniser has the original arg expressions, this path only has the
/// already-lowered HIR — so we always emit the general HIR variant which
/// preserves spec semantics at runtime, just without the constant-fold
/// optimisations.
///
/// The whitelist here MUST stay in sync with the `is_supported` filter in
/// `destructuring.rs::lower_var_decl_with_destructuring` — methods missing
/// from the dispatch below will be tagged as aliases and then fall through
/// to the original generic call path that throws `TypeError: value is not
/// a function`, regressing the very pattern this fix is supposed to fix.
pub(super) fn build_object_static_method_call(method: &str, args: Vec<Expr>) -> Expr {
    let mut iter = args.into_iter();
    match method {
        "defineProperty" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            let descriptor = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectDefineProperty(Box::new(obj), Box::new(key), Box::new(descriptor))
        }
        "defineProperties" => {
            let target = iter.next().unwrap_or(Expr::Undefined);
            let descs = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectDefineProperties(Box::new(target), Box::new(descs))
        }
        "setPrototypeOf" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let proto = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectSetPrototypeOf(Box::new(obj), Box::new(proto))
        }
        "getPrototypeOf" => {
            Expr::ObjectGetPrototypeOf(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "getOwnPropertyDescriptor" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectGetOwnPropertyDescriptor(Box::new(obj), Box::new(key))
        }
        "getOwnPropertyDescriptors" => {
            Expr::ObjectGetOwnPropertyDescriptors(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "getOwnPropertyNames" => {
            Expr::ObjectGetOwnPropertyNames(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "getOwnPropertySymbols" => {
            Expr::ObjectGetOwnPropertySymbols(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "keys" => Expr::ObjectKeys(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "values" => Expr::ObjectValues(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "entries" => Expr::ObjectEntries(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "assign" => {
            let target = iter.next().unwrap_or(Expr::Undefined);
            let sources: Vec<Expr> = iter.collect();
            Expr::ObjectAssign {
                target: Box::new(target),
                sources,
            }
        }
        "fromEntries" => Expr::ObjectFromEntries(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "create" => {
            let proto = iter.next().unwrap_or(Expr::Undefined);
            let props = iter.next().map(Box::new);
            Expr::ObjectCreate(Box::new(proto), props)
        }
        "freeze" => Expr::ObjectFreeze(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "seal" => Expr::ObjectSeal(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "preventExtensions" => {
            Expr::ObjectPreventExtensions(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "isFrozen" => Expr::ObjectIsFrozen(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "isSealed" => Expr::ObjectIsSealed(Box::new(iter.next().unwrap_or(Expr::Undefined))),
        "isExtensible" => {
            Expr::ObjectIsExtensible(Box::new(iter.next().unwrap_or(Expr::Undefined)))
        }
        "hasOwn" => {
            let obj = iter.next().unwrap_or(Expr::Undefined);
            let key = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectHasOwn(Box::new(obj), Box::new(key))
        }
        "is" => {
            let a = iter.next().unwrap_or(Expr::Undefined);
            let b = iter.next().unwrap_or(Expr::Undefined);
            Expr::ObjectIs(Box::new(a), Box::new(b))
        }
        // Unreachable in practice — the whitelist in destructuring.rs
        // gates which methods reach this dispatch. The fall-through is
        // defensive: an unrecognised method shouldn't have been tagged
        // as an alias, but if it slips through we emit Undefined rather
        // than panic-ing the compiler.
        _ => Expr::Undefined,
    }
}
