//! Constructor super lowering when the live heritage can change.
#[cfg(test)]
#[path = "super_constructor_no_heritage_tests.rs"]
mod no_heritage_tests;

use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64, PTR};
use anyhow::Result;
use perry_hir::{Class, Expr};

/// Type specialization changes the instance layout, not the lexical class
/// whose [[Prototype]] super() observes. Only its source declaration evaluates
/// heritage and can be named by a JavaScript prototype write.
pub(crate) fn lexical_class_id(ctx: &FnCtx<'_>, class: &Class) -> u32 {
    let mut owner = class;
    for _ in 0..ctx.classes.len() {
        let Some(parent) = owner
            .specialized_from
            .as_deref()
            .and_then(|name| ctx.classes.get(name).copied())
        else {
            break;
        };
        owner = parent;
    }
    ctx.class_ids.get(&owner.name).copied().unwrap_or(owner.id)
}

/// A direct super body may skip default constructors only while every skipped
/// constructor has a whole-program no-relink proof. An own body handles its
/// next edge itself, so it terminates this walk.
pub(crate) fn requires_live(ctx: &FnCtx<'_>, class: &Class) -> bool {
    // A class with no heritage has no super constructor, so nothing about a
    // live-changing parent applies. The lookups below are keyed on a bare
    // class name, and a type-only import cycle can register an entry under
    // the name of a class this module DECLARES (#12889), which made an
    // undeclared parent look live and emitted a super-construct that throws
    // `Super constructor is not a constructor` at run time.
    if class.extends_name.is_none()
        && class.extends_expr.is_none()
        && class.extends.is_none()
        && class.native_extends.is_none()
    {
        return false;
    }
    if ctx
        .imported_class_ctors
        .get(&class.name)
        .is_some_and(|c| !c.has_own_constructor)
    {
        return true;
    }
    if class.extends_name.as_deref().is_some_and(|parent| {
        ctx.imported_class_ctors
            .get(parent)
            .is_some_and(|c| !c.has_own_constructor)
    }) {
        return true;
    }
    let mut owner = class;
    for _ in 0..64 {
        let derived = owner.extends_name.is_some()
            || owner.extends_expr.is_some()
            || owner.extends.is_some()
            || owner.native_extends.is_some();
        if !derived {
            return false;
        }
        // A WHATWG stream base has no constructible parent VALUE: its state is
        // installed on `this` by the dedicated `super(...)` init
        // (`lower_stream_super_init`), and that edge is the construction.
        if web_stream_parent(owner) {
            return false;
        }
        if !crate::proto_constants::heritage_is_unrelinked(ctx, lexical_class_id(ctx, owner)) {
            return true;
        }
        let Some(parent) = owner
            .extends_name
            .as_deref()
            .and_then(|p| ctx.classes.get(p).copied())
        else {
            return false;
        };
        if parent.constructor.is_some()
            || ctx
                .imported_class_ctors
                .get(&parent.name)
                .is_some_and(|ctor| ctor.has_own_constructor)
        {
            return false;
        }
        owner = parent;
    }
    true
}

/// `class X extends ReadableStream / WritableStream / TransformStream`.
fn web_stream_parent(class: &Class) -> bool {
    class.native_extends.as_ref().is_some_and(|(module, _)| {
        matches!(
            module.as_str(),
            "readable_stream" | "writable_stream" | "transform_stream"
        )
    })
}

/// A default descendant must not inline an ancestor body already being lowered.
/// The ancestor has temporarily replaced the leaf on class_stack.
pub(crate) fn default_body_is_active(ctx: &FnCtx<'_>, class: &Class) -> bool {
    let mut owner = class;
    for _ in 0..ctx.classes.len() {
        if owner.constructor.is_some() {
            let id = lexical_class_id(ctx, owner);
            return ctx.class_stack.iter().any(|name| {
                ctx.classes
                    .get(name)
                    .is_some_and(|c| lexical_class_id(ctx, c) == id)
            });
        }
        let Some(parent) = owner
            .extends_name
            .as_deref()
            .and_then(|n| ctx.classes.get(n).copied())
        else {
            break;
        };
        owner = parent;
    }
    false
}

pub(crate) fn call(ctx: &mut FnCtx<'_>, class: &Class, args: &[Expr]) -> Result<String> {
    if let Some(plan) = super::checked_super::plan(ctx, class) {
        return super::checked_super::call(ctx, class, args, &plan);
    }
    call_live(ctx, class, args)
}

pub(crate) fn call_live(ctx: &mut FnCtx<'_>, class: &Class, args: &[Expr]) -> Result<String> {
    crate::rooting::with_rooted_group(ctx, args.len() + 1, |ctx, group| {
        let mut ids = Vec::new();
        for arg in args {
            ids.push(group.lower(ctx, arg, true)?);
        }
        // GetSuperConstructor observes prototype writes performed by arguments.
        let lexical_id = lexical_class_id(ctx, class);
        let parent = ctx.block().call(
            DOUBLE,
            "js_class_super_parent",
            &[(I32, &lexical_id.to_string())],
        );
        let parent = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &parent, true);
        // Preparation can collect: only now reread and pack rooted arguments.
        let values = ids
            .into_iter()
            .map(|id| group.reread(ctx, id))
            .collect::<Result<Vec<_>>>()?;
        let (ptr, len) = crate::lower_call::emit_method_args_buffer(ctx, &values);
        let save = crate::rooting::new_target_save_for_super(ctx);
        let site = super::new_dynamic::construct_site(ctx);
        let this = ctx
            .this_stack
            .last()
            .cloned()
            .map(|slot| ctx.block().load(DOUBLE, &slot))
            .unwrap_or_else(|| {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            });
        let parent = group.reread_emitted(ctx, parent);
        let result = ctx.block().call(
            DOUBLE,
            "js_class_value_super_construct_site",
            &[
                (DOUBLE, &parent),
                (DOUBLE, &this),
                (PTR, &ptr),
                (I64, &len),
                (PTR, &site),
            ],
        );
        if let Some(save) = save.as_ref() {
            crate::rooting::new_target_restore(ctx, save);
        }
        bind(ctx, &result);
        crate::lower_call::apply_field_initializers_recursive(
            ctx,
            &class.name,
            crate::lower_call::FieldInitMode::SelfOnly,
        )?;
        Ok(ctx
            .this_stack
            .last()
            .cloned()
            .map(|slot| ctx.block().load(DOUBLE, &slot))
            .unwrap_or(result))
    })
}

pub(crate) fn bind(ctx: &mut FnCtx<'_>, result: &str) {
    super::this_super_call::bind_derived_this_after_super(ctx);
    if let Some(slot) = ctx.this_stack.last().cloned() {
        let this = ctx.block().load(DOUBLE, &slot);
        let value = crate::lower_call::emit_ctor_return_override(ctx, &this, result, false);
        super::this_super_call::store_returned_this(ctx, &slot, &this, &value);
    }
}

/// Route a mutable default-ctor chain through its own registered constructor;
/// it must not be collapsed to the definition-time ancestor at the new site.
pub(crate) fn new(ctx: &mut FnCtx<'_>, name: &str, args: &[Expr]) -> Result<String> {
    // Imported stubs all have id 0. Preserve the requested binding (including
    // namespace qualification), following only explicit specialization names.
    let mut lexical_name = name;
    for _ in 0..ctx.classes.len() {
        let Some(origin) = ctx
            .classes
            .get(lexical_name)
            .and_then(|class| class.specialized_from.as_deref())
        else {
            break;
        };
        lexical_name = origin;
    }
    let callee = Expr::ClassRef(lexical_name.to_owned());
    crate::rooting::with_rooted_group(ctx, args.len() + 1, |ctx, group| {
        let f = lower_expr(ctx, &callee)?;
        let f = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &f, true);
        let mut ids = Vec::new();
        for arg in args {
            ids.push(group.lower(ctx, arg, true)?);
        }
        let values = ids
            .into_iter()
            .map(|id| group.reread(ctx, id))
            .collect::<Result<Vec<_>>>()?;
        let (ptr, len) = crate::lower_call::emit_method_args_buffer(ctx, &values);
        let site = super::new_dynamic::construct_site(ctx);
        let f = group.reread_emitted(ctx, f);
        Ok(ctx.block().call(
            DOUBLE,
            "js_new_function_construct_site",
            &[(DOUBLE, &f), (PTR, &ptr), (I64, &len), (PTR, &site)],
        ))
    })
}
