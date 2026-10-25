//! Decision 150 checked heritage: compare the constructor's actual own edge,
//! then use the original direct lowering only on the matching control-flow arm.
use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};
use anyhow::Result;
use perry_hir::{Class, Expr};

pub(crate) struct Plan {
    edges: Vec<(u32, u32)>,
    first_nested: bool,
}

pub(crate) fn plan(ctx: &FnCtx<'_>, class: &Class) -> Option<Plan> {
    let mut owner = class;
    let mut edges = Vec::new();
    let mut complete = false;
    for _ in 0..ctx.classes.len().min(64) {
        if owner.heritage_lexically_shadowed
            || owner.extends_expr.is_some()
            || owner.native_extends.is_some()
        {
            return None;
        }
        let Some(parent_name) = owner.extends_name.as_deref() else {
            if owner.is_imported_stub() {
                return None;
            }
            complete = true;
            break;
        };
        let parent = *ctx.classes.get(parent_name)?;
        // Ancestor field expressions are lowered before binding the parent's
        // constructor parameters. A captured local absent from this frame
        // must stay with its defining constructor's generic entry.
        if !ancestor_fields_in_scope(ctx, parent) {
            return None;
        }
        let cid = super::super_constructor::lexical_class_id(ctx, owner);
        // Specialization changes the direct body, never the constructor edge.
        let parent_id = super::super_constructor::lexical_class_id(ctx, parent);
        if cid == 0 || parent_id == 0 {
            return None;
        }
        edges.push((cid, parent_id));
        if parent.constructor.is_some()
            || ctx
                .imported_class_ctors
                .get(parent_name)
                .is_some_and(|ctor| ctor.has_own_constructor)
        {
            complete = true;
            break;
        }
        owner = parent;
    }
    // A partial chain cannot justify skipping the remaining default bodies.
    (complete && !edges.is_empty()).then_some(Plan {
        edges,
        first_nested: class.is_nested,
    })
}

fn ancestor_fields_in_scope(ctx: &FnCtx<'_>, class: &Class) -> bool {
    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for field in &class.fields {
        if field.origin.is_reservation()
            || field.name.starts_with("__perry_cap_")
        {
            continue;
        }
        for expr in field.init.iter().chain(field.key_expr.iter()) {
            perry_hir::collect_local_refs_expr(expr, &mut refs, &mut visited);
        }
    }
    refs.iter().all(|id| {
        ctx.locals.contains_key(id)
            || ctx.module_globals.contains_key(id)
            || ctx.closure_captures.contains_key(id)
    })
}

fn guard(ctx: &mut FnCtx<'_>, plan: &Plan, current_evaluation: bool) -> String {
    let mut matches = "true".to_owned();
    for (index, &(cid, parent_id)) in plan.edges.iter().enumerate() {
        // A super call belongs to the active evaluation of a nested class.
        // A construct site's static fast path instead names its shared class
        // object. Once an edge matches, the next owner is canonical too.
        let actual = if index == 0 && plan.first_nested && current_evaluation {
            ctx.block()
                .call(DOUBLE, "js_class_super_parent", &[(I32, &cid.to_string())])
        } else {
            let constructor = super::emit_class_value(ctx.block(), cid);
            let bits = ctx.block().bitcast_double_to_i64(&constructor);
            let pointer = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
            let offset = crate::target_layout::closure_header_size_bytes(ctx.target_triple)
                + 8 * crate::runtime_abi::CLASS_CONSTRUCTOR_PARENT_CAPTURE as u64;
            let address = ctx.block().add(I64, &pointer, &offset.to_string());
            let address = ctx.block().inttoptr(I64, &address);
            ctx.block().load(DOUBLE, &address)
        };
        let expected = super::emit_class_value(ctx.block(), parent_id);
        let actual = ctx.block().bitcast_double_to_i64(&actual);
        let expected = ctx.block().bitcast_double_to_i64(&expected);
        let equal = ctx.block().icmp_eq(I64, &actual, &expected);
        matches = ctx.block().and(I1, &matches, &equal);
    }
    matches
}

fn branch(
    ctx: &mut FnCtx<'_>,
    plan: &Plan,
    current_evaluation: bool,
    fast: impl FnOnce(&mut FnCtx<'_>) -> Result<String>,
    slow: impl FnOnce(&mut FnCtx<'_>) -> Result<String>,
) -> Result<String> {
    let equal = guard(ctx, plan, current_evaluation);
    let direct = ctx.new_block("super.checked.direct");
    let live = ctx.new_block("super.checked.live");
    let merge = ctx.new_block("super.checked.merge");
    let dl = ctx.block_label(direct);
    let ll = ctx.block_label(live);
    let ml = ctx.block_label(merge);
    ctx.block().cond_br(&equal, &dl, &ll);
    ctx.current_block = direct;
    let direct_value = fast(ctx)?;
    let direct_pred = ctx.block().label.clone();
    let direct_reaches = !ctx.block().is_terminated();
    if direct_reaches {
        ctx.block().br(&ml);
    }
    ctx.current_block = live;
    let live_value = slow(ctx)?;
    let live_pred = ctx.block().label.clone();
    let live_reaches = !ctx.block().is_terminated();
    if live_reaches {
        ctx.block().br(&ml);
    }
    ctx.current_block = merge;
    let mut incoming = Vec::new();
    if direct_reaches {
        incoming.push((direct_value.as_str(), direct_pred.as_str()));
    }
    if live_reaches {
        incoming.push((live_value.as_str(), live_pred.as_str()));
    }
    Ok(ctx.block().phi(DOUBLE, &incoming))
}

/// Plain local/literal reads cannot relink heritage. Keep their original
/// expression identities so the existing direct lowering retains its argument
/// and allocation proofs. Effectful arguments must run and be rooted first.
fn with_arguments(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
    body: impl FnOnce(&mut FnCtx<'_>, &[Expr]) -> Result<String>,
) -> Result<String> {
    if args.iter().all(|arg| {
        matches!(
            arg,
            Expr::LocalGet(_)
                | Expr::Number(_)
                | Expr::String(_)
                | Expr::Bool(_)
                | Expr::Null
                | Expr::Undefined
        )
    }) {
        body(ctx, args)
    } else {
        crate::rooting::with_scoped_arguments(ctx, args, body)
    }
}

pub(crate) fn new(ctx: &mut FnCtx<'_>, name: &str, args: &[Expr], plan: &Plan) -> Result<String> {
    with_arguments(ctx, args, |ctx, args| {
        branch(
            ctx,
            plan,
            false,
            |ctx| crate::lower_call::lower_new_checked(ctx, name, args),
            |ctx| super::super_constructor::new(ctx, name, args),
        )
    })
}

pub(crate) fn call(
    ctx: &mut FnCtx<'_>,
    class: &Class,
    args: &[Expr],
    plan: &Plan,
) -> Result<String> {
    with_arguments(ctx, args, |ctx, args| {
        branch(
            ctx,
            plan,
            true,
            |ctx| {
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    &class.name,
                    crate::lower_call::FieldInitMode::AncestorsOnly,
                )?;
                super::this_super_call::lower_checked(ctx, &Expr::SuperCall(args.to_vec()))
            },
            |ctx| super::super_constructor::call_live(ctx, class, args),
        )
    })
}

/// Share large parent bodies at a standalone default-derived entry. Its
/// caller already packs the parent's rest/arguments ABI. Capturing parents
/// keep the existing binding path; their snapshots are not plain arguments.
/// Small parents retain the inline context used by the ordinary lowering.
pub(crate) fn default_call(ctx: &mut FnCtx<'_>, class: &Class, args: &[Expr]) -> Result<String> {
    let parent = class
        .extends_name
        .as_deref()
        .and_then(|name| ctx.classes.get(name).copied());
    let target = parent.and_then(|parent| {
        let ctor = parent.constructor.as_ref()?;
        // Match the ordinary function inliner's eight-statement budget.
        // Sharing small parents loses context for little code-size benefit.
        if ctor.body.len() <= 8
            || ctor
                .params
                .iter()
                .any(|p| p.name.starts_with("__perry_cap_"))
        {
            return None;
        }
        (ctor.params.len() == args.len())
            .then(|| crate::symbol_names::constructor(&ctx.strings.module_prefix(), &parent.name))
    });
    let Some((plan, target)) = plan(ctx, class).zip(target) else {
        return super::super_constructor::call(ctx, class, args);
    };
    branch(
        ctx,
        &plan,
        true,
        |ctx| {
            // No ancestor field staging here: the parent's standalone entry
            // owns its fields. Derived fields follow the successful binding.
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(super::lower_expr(ctx, arg)?);
            }
            let slot = ctx
                .this_stack
                .last()
                .expect("default constructor this")
                .clone();
            let this = ctx.block().load(DOUBLE, &slot);
            let mut call_args = vec![(DOUBLE, this.as_str())];
            call_args.extend(values.iter().map(|value| (DOUBLE, value.as_str())));
            ctx.pending_declares
                .push((target.clone(), DOUBLE, vec![DOUBLE; args.len() + 1]));
            let result = ctx.block().call(DOUBLE, &target, &call_args);
            super::super_constructor::bind(ctx, &result);
            crate::lower_call::apply_field_initializers_recursive(
                ctx,
                &class.name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )?;
            Ok(ctx.block().load(DOUBLE, &slot))
        },
        |ctx| super::super_constructor::call_live(ctx, class, args),
    )
}

/// Spread operands have already been iterated once. Guard the resulting list
/// and pass that same list to either dispatcher; never replay user iteration.
pub(crate) fn spread(
    ctx: &mut FnCtx<'_>,
    class: &Class,
    array: &str,
    plan: &Plan,
) -> Result<String> {
    crate::rooting::with_rooted_group(ctx, 1, |ctx, group| {
        let array = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, array, true);
        branch(
            ctx,
            plan,
            true,
            |ctx| {
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    &class.name,
                    crate::lower_call::FieldInitMode::AncestorsOnly,
                )?;
                let cid = super::super_constructor::lexical_class_id(ctx, class);
                let array = group.reread_emitted(ctx, array);
                let slot = ctx.this_stack.last().unwrap().clone();
                let this = ctx.block().load(DOUBLE, &slot);
                let save = crate::rooting::new_target_save_for_super(ctx);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_super_construct_apply",
                    &[(I32, &cid.to_string()), (DOUBLE, &this), (DOUBLE, &array)],
                );
                if let Some(save) = save.as_ref() {
                    crate::rooting::new_target_restore(ctx, save);
                }
                super::super_constructor::bind(ctx, &result);
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    &class.name,
                    crate::lower_call::FieldInitMode::SelfOnly,
                )?;
                Ok(ctx.block().load(DOUBLE, &slot))
            },
            |ctx| {
                let cid = super::super_constructor::lexical_class_id(ctx, class);
                let parent =
                    ctx.block()
                        .call(DOUBLE, "js_class_super_parent", &[(I32, &cid.to_string())]);
                let array = group.reread_emitted(ctx, array);
                let slot = ctx.this_stack.last().unwrap().clone();
                let this = ctx.block().load(DOUBLE, &slot);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_class_super_construct_apply",
                    &[(DOUBLE, &parent), (DOUBLE, &this), (DOUBLE, &array)],
                );
                super::super_constructor::bind(ctx, &result);
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    &class.name,
                    crate::lower_call::FieldInitMode::SelfOnly,
                )?;
                Ok(ctx.block().load(DOUBLE, &slot))
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::{compile_module, CompileOptions};

    fn ir(params: &str, statements: usize) -> String {
        let parent_body = "parentBody(value);".repeat(statements);
        let source = format!(
            "function parentBody(x: any) {{ return x; }}
             export class Base {{
               constructor({params}) {{ {parent_body} }}
             }}
             export class First extends Base {{}}
             export class Second extends Base {{}}"
        );
        let ast = perry_parser::parse_typescript(&source, "spx_shared.ts").unwrap();
        let module = perry_hir::lower_module(&ast, "spx_shared", "spx_shared.ts").unwrap();
        let census = perry_hir::ProtoCensus {
            relink: true,
            ..Default::default()
        };
        crate::proto_constants::with_test_census(census, || {
            String::from_utf8(
                compile_module(
                    &module,
                    CompileOptions {
                        emit_ir_only: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap()
        })
    }

    fn body<'a>(ir: &'a str, class: &str) -> &'a str {
        let symbol = format!("__{class}_constructor(");
        let start = ir
            .lines()
            .find(|line| line.starts_with("define ") && line.contains(&symbol))
            .expect("standalone constructor");
        let start = ir.find(start).unwrap();
        &ir[start..start + ir[start..].find("\n}").unwrap()]
    }

    fn check_shared(ir: &str) {
        let parent = body(ir, "Base");
        assert!(
            parent.contains("parentBody"),
            "fixture must contain a parent body"
        );
        for class in ["First", "Second"] {
            let child = body(ir, class);
            assert!(child.contains("super.checked.direct"), "{child}");
            assert!(child.contains("super.checked.live"), "{child}");
            assert!(
                child.contains("icmp eq i64"),
                "live edge must still be checked: {child}"
            );
            assert!(child.contains("@spx_shared__Base_constructor("), "{child}");
            assert!(
                child.contains("@js_class_value_super_construct_site("),
                "{child}"
            );
            assert!(
                !child.contains("parentBody"),
                "parent body was replicated: {child}"
            );
            assert!(
                !child.contains("@js_array_alloc("),
                "forwarded arrays were repacked: {child}"
            );
        }
    }

    #[test]
    fn default_derived_entries_share_the_checked_parent_body() {
        check_shared(&ir("value: any", 16));
    }

    #[test]
    fn default_derived_entries_forward_the_parents_packed_rest_abi() {
        check_shared(&ir("value: any, ...rest: any[]", 16));
    }
    #[test]
    fn small_parents_keep_the_checked_inline_context() {
        let ir = ir("value: any", 1);
        for class in ["First", "Second"] {
            let child = body(&ir, class);
            assert!(child.contains("super.checked.direct"), "{child}");
            assert!(child.contains("super.checked.live"), "{child}");
            assert!(child.contains("parentBody"), "{child}");
            assert!(!child.contains("@spx_shared__Base_constructor("), "{child}");
        }
    }

    #[test]
    fn capture_slots_keep_the_existing_snapshot_binding() {
        let ir = ir("value: any, __perry_cap_77: any", 16);
        for class in ["First", "Second"] {
            let child = body(&ir, class);
            assert!(child.contains("super.checked.direct"), "{child}");
            assert!(child.contains("parentBody"), "{child}");
            assert!(child.contains("@js_class_capture_value_or("), "{child}");
            assert!(!child.contains("@spx_shared__Base_constructor("), "{child}");
        }
    }
}
