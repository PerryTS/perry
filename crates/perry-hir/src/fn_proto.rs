//! Which intrinsic prototype a function object's chain starts at: the one
//! predicate codegen (the function info flags the runtime births the
//! object's ShapeId from) and the whole-program solver (DEC 250: a chain
//! that is `Function.prototype` only is add-locked) both read.
//!
//! It reads the source kind (`is_async`, `is_generator`) and, on the
//! lowered module, what the transforms leave of it: the generator transform turns a generator
//! function into a wrapper that returns its generator object, and clears
//! `is_async` on async generators (`Module::async_generator_funcs` keeps
//! them); the await lowering rewrites an async body into a step machine
//! (`Function::was_plain_async`, `Module::async_step_closures`). An async
//! generator's step body births its record with `js_async_generator_create`
//! (`GeneratorRecordKind::Async`), which names the kind as well.
//!
//! Codegen reads it for every function object it describes: declarations and
//! closures (`artifacts.rs`), class method entries (`method_entries.rs`); no
//! codegen reader keeps its own reading of the source flags.

use crate::ir::{Expr, Function, Module, Stmt};
use crate::types::FuncId;

/// The intrinsic prototype a function object is born with.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FnProtoKind {
    /// `%Function.prototype%` (plain functions, arrows, methods, accessors).
    Function,
    /// `%AsyncFunction.prototype%`.
    Async,
    /// `%GeneratorFunction.prototype%`.
    Generator,
    /// `%AsyncGeneratorFunction.prototype%`.
    AsyncGenerator,
}

impl FnProtoKind {
    /// A function declaration or class member of `module`.
    pub fn of_function(module: &Module, f: &Function) -> Self {
        if module.async_generator_funcs.contains(&f.id)
            || (f.is_async && f.is_generator)
            || body_generator_record_kind(&f.body) == Some(GeneratorRecordKind::Async)
        {
            FnProtoKind::AsyncGenerator
        } else if f.is_async || f.was_plain_async {
            FnProtoKind::Async
        } else if f.is_generator || body_is_generator_wrapper(&f.body) {
            FnProtoKind::Generator
        } else {
            FnProtoKind::Function
        }
    }

    /// A closure expression of `module` (`Expr::Closure`'s `func_id`,
    /// `is_async`, `is_generator` and `body`).
    pub fn of_closure(
        module: &Module,
        func_id: FuncId,
        is_async: bool,
        is_generator: bool,
        body: &[Stmt],
    ) -> Self {
        if module.async_generator_funcs.contains(&func_id)
            || (is_async && is_generator)
            || body_generator_record_kind(body) == Some(GeneratorRecordKind::Async)
        {
            FnProtoKind::AsyncGenerator
        } else if is_async || module.async_step_closures.contains(&func_id) {
            FnProtoKind::Async
        } else if is_generator || body_is_generator_wrapper(body) {
            FnProtoKind::Generator
        } else {
            FnProtoKind::Function
        }
    }
}

/// A generator birth that owns its resume record: a sync generator's
/// fixed-state record, or an async generator's step body
/// (`js_async_generator_create`). Both link the instance to the function's
/// prototype themselves and retain the owner function.
pub fn is_record_generator_birth(expr: &Expr) -> bool {
    record_generator_birth_kind(expr).is_some()
}

/// Which generator function kind a record birth belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorRecordKind {
    Sync,
    Async,
}

/// The kind of a record birth: the birth itself says whether the body is an
/// async generator (`js_async_generator_create`) or a sync one.
pub fn record_generator_birth_kind(expr: &Expr) -> Option<GeneratorRecordKind> {
    match expr {
        Expr::LinkGeneratorPrototype {
            fixed_state: true, ..
        } => Some(GeneratorRecordKind::Sync),
        Expr::LinkGeneratorPrototype {
            is_async: true,
            obj,
            ..
        } if matches!(obj.as_ref(), Expr::Closure { .. }) => Some(GeneratorRecordKind::Async),
        _ => None,
    }
}

/// The record-birth kind of a generator wrapper body, if it returns one.
pub fn body_generator_record_kind(body: &[Stmt]) -> Option<GeneratorRecordKind> {
    body.iter().find_map(|stmt| match stmt {
        Stmt::Return(Some(e)) => record_generator_birth_kind(e),
        _ => None,
    })
}

/// A lowered generator wrapper: its body returns its generator object,
/// a record birth or the `{next, return, throw}` step object.
pub fn body_is_generator_wrapper(body: &[Stmt]) -> bool {
    body_generator_record_kind(body).is_some() || function_body_returns_generator_object(body)
}

/// The generator transform's wrapper with a `__gen_state` local: its body
/// returns the generator object (a record birth, or the `{next, return,
/// throw}` step object, possibly inside the instance-prototype linker).
pub fn function_body_returns_generator_object(body: &[Stmt]) -> bool {
    let has_gen_state = body
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Let { name, .. } if name == "__gen_state"));
    if !has_gen_state {
        return false;
    }
    body.iter().any(|stmt| match stmt {
        Stmt::Return(Some(e)) if is_record_generator_birth(e) => true,
        // The generator transform may wrap the returned iterator in the
        // instance-prototype linker; unwrap it so the iterator shape remains
        // the stable signal that this is a lowered generator wrapper.
        Stmt::Return(Some(expr)) => {
            let inner = match expr {
                Expr::LinkGeneratorPrototype { obj, .. } => obj.as_ref(),
                other => other,
            };
            matches!(inner, Expr::Object(props)
                if props.len() == 3
                    && props[0].0 == "next"
                    && props[1].0 == "return"
                    && props[2].0 == "throw"
                    && props
                        .iter()
                        .all(|(_, value)| matches!(value, Expr::Closure { .. })))
        }
        _ => false,
    })
}
