//! URLSearchParams method-call HIR builder.
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the only consumer is `lower_call_inner` inside this module.

use crate::ir::*;

/// Build the `Expr::UrlSearchParams*` HIR variant for `<recv>.<method>(args)`.
/// Returns the populated `args` back when the (method, arity) combo doesn't
/// match a known URLSearchParams op so the caller can fall through.
///
/// Shared between the typed-local arm and the chained-`new URLSearchParams(...)`
/// arm so both surfaces dispatch identically (refs #575).
pub(super) fn build_url_search_params_method_call(
    recv: Expr,
    method: &str,
    args: Vec<Expr>,
) -> std::result::Result<Expr, Vec<Expr>> {
    match method {
        "get" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsGet {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "has" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsHas {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "set" if args.len() >= 2 => {
            let mut iter = args.into_iter();
            let name = iter.next().unwrap();
            let value = iter.next().unwrap();
            Ok(Expr::UrlSearchParamsSet {
                params: Box::new(recv),
                name: Box::new(name),
                value: Box::new(value),
            })
        }
        "append" if args.len() >= 2 => {
            let mut iter = args.into_iter();
            let name = iter.next().unwrap();
            let value = iter.next().unwrap();
            Ok(Expr::UrlSearchParamsAppend {
                params: Box::new(recv),
                name: Box::new(name),
                value: Box::new(value),
            })
        }
        "delete" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsDelete {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        "toString" => Ok(Expr::UrlSearchParamsToString(Box::new(recv))),
        "getAll" if !args.is_empty() => {
            let name = args.into_iter().next().unwrap();
            Ok(Expr::UrlSearchParamsGetAll {
                params: Box::new(recv),
                name: Box::new(name),
            })
        }
        _ => Err(args),
    }
}
