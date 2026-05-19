//! `Reflect.*Metadata` argument-shape helpers.
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the only consumers are inside `lower_call_inner` in this module.

use crate::ir::*;

/// (key, value, target, propertyKey?) — `Reflect.defineMetadata`'s 3-or-4 arg
/// shape. Defaults missing leading args to `undefined`; `property_key` stays
/// `None` when omitted so the runtime can distinguish class-level metadata
/// from a property-level one.
pub(super) fn take_reflect_kvtp_args(args: Vec<Expr>) -> (Expr, Expr, Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let key = it.next().unwrap_or(Expr::Undefined);
    let value = it.next().unwrap_or(Expr::Undefined);
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (key, value, target, property_key)
}

/// (key, target, propertyKey?) — `Reflect.{get,getOwn,has,hasOwn,delete}Metadata`.
pub(super) fn take_reflect_ktp_args(args: Vec<Expr>) -> (Expr, Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let key = it.next().unwrap_or(Expr::Undefined);
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (key, target, property_key)
}

/// (target, propertyKey?) — `Reflect.{get,getOwn}MetadataKeys`.
pub(super) fn take_reflect_tp_args(args: Vec<Expr>) -> (Expr, Option<Box<Expr>>) {
    let mut it = args.into_iter();
    let target = it.next().unwrap_or(Expr::Undefined);
    let property_key = it.next().map(Box::new);
    (target, property_key)
}
