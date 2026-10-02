//! Static ConstFn describes a completed object. Allocation ids remain Any/F64.
use super::{BirthProto, BirthShape, CompileOptions, ConstFnBirth, ModuleBirth};
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{BTreeSet, HashMap};

pub(crate) fn enabled(opts: &CompileOptions) -> bool {
    opts.output_type == "executable" && std::env::var("PERRY_CONSTFN_SHAPE").as_deref() == Ok("1")
}

pub(crate) fn literal_final(
    prefix: &str,
    props: &[(String, Expr)],
    base_rep: u64,
) -> Option<BirthShape> {
    if props.is_empty() || props.len() > 32 {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    let mut rep = base_rep;
    let mut constfn = Vec::new();
    for (slot, (key, value)) in props.iter().enumerate() {
        if key.is_empty() || key.contains('\0') || key == "__proto__" || !seen.insert(key) {
            return None;
        }
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        if let Expr::Closure {
            func_id,
            params,
            is_async: false,
            is_generator: false,
            captures_this,
            is_arrow,
            ..
        } = value
        {
            // Rebindable method clones cannot satisfy the direct body ABI.
            if (*captures_this && !*is_arrow)
                || params
                    .iter()
                    .any(|p| p.is_rest || p.arguments_object.is_some())
            {
                continue;
            }
            if (base_rep >> (slot * 2)) & 3 != 0 {
                return None;
            }
            rep |= 3 << (slot * 2);
            constfn.push(ConstFnBirth {
                slot: slot as u8,
                symbol: crate::fn_info::info_symbol(&crate::fn_info::closure_body_symbol(
                    prefix, *func_id,
                )),
            });
        }
    }
    if constfn.is_empty() {
        return None;
    }
    Some(BirthShape {
        keys,
        key_count: props.len() as u32,
        live: props.len() as u32,
        proto: BirthProto::Literal,
        typed: None,
        rep,
        constfn,
    })
}

/// Only the exact synthetic record constructor is admitted. Arbitrary classes,
/// heritage, descriptors, computed keys and early returns fail closed.
pub(crate) fn anon_props(class: &Class, args: &[Expr]) -> Option<Vec<(String, Expr)>> {
    if !class.is_literal_shape() || class.fields.len() != args.len() {
        return None;
    }
    Some(
        class
            .fields
            .iter()
            .zip(args)
            .map(|(f, value)| (f.name.clone(), value.clone()))
            .collect(),
    )
}

pub(crate) fn module_literal_finals(
    module: &Module,
    prefix: &str,
    class_reps: &HashMap<String, u64>,
) -> Vec<ModuleBirth> {
    fn expr(
        e: &Expr,
        module: &Module,
        prefix: &str,
        reps: &HashMap<String, u64>,
        out: &mut BTreeSet<BirthShape>,
    ) {
        let shape = match e {
            Expr::Object(props) => literal_final(prefix, props, 0),
            Expr::New {
                class_name,
                args,
                cap_args_appended: 0,
                ..
            } => module
                .classes
                .iter()
                .find(|c| &c.name == class_name)
                .and_then(|c| anon_props(c, args))
                .and_then(|props| literal_final(prefix, &props, *reps.get(class_name)?)),
            _ => None,
        };
        if let Some(shape) = shape {
            out.insert(shape);
        }
        if let Expr::Closure { body, .. } = e {
            stmts(body, module, prefix, reps, out);
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| {
            expr(child, module, prefix, reps, out)
        });
    }
    fn stmts(
        body: &[Stmt],
        m: &Module,
        p: &str,
        r: &HashMap<String, u64>,
        out: &mut BTreeSet<BirthShape>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Let { init, .. } | Stmt::Return(init) => {
                    if let Some(e) = init {
                        expr(e, m, p, r, out);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => expr(e, m, p, r, out),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    expr(condition, m, p, r, out);
                    stmts(then_branch, m, p, r, out);
                    if let Some(b) = else_branch {
                        stmts(b, m, p, r, out);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                    expr(condition, m, p, r, out);
                    stmts(body, m, p, r, out);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(s) = init {
                        stmts(std::slice::from_ref(s), m, p, r, out);
                    }
                    for e in [condition, update].into_iter().flatten() {
                        expr(e, m, p, r, out);
                    }
                    stmts(body, m, p, r, out);
                }
                Stmt::Labeled { body, .. } => stmts(std::slice::from_ref(body), m, p, r, out),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    stmts(body, m, p, r, out);
                    if let Some(c) = catch {
                        stmts(&c.body, m, p, r, out);
                    }
                    if let Some(b) = finally {
                        stmts(b, m, p, r, out);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    expr(discriminant, m, p, r, out);
                    for c in cases {
                        if let Some(e) = &c.test {
                            expr(e, m, p, r, out);
                        }
                        stmts(&c.body, m, p, r, out);
                    }
                }
                Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_)
                | Stmt::ReleaseBoxes(_) => {}
            }
        }
    }
    let mut out = BTreeSet::new();
    stmts(&module.init, module, prefix, class_reps, &mut out);
    for global in &module.globals {
        if let Some(e) = &global.init {
            expr(e, module, prefix, class_reps, &mut out);
        }
    }
    for f in &module.functions {
        stmts(&f.body, module, prefix, class_reps, &mut out);
    }
    for c in &module.classes {
        for f in c
            .constructor
            .iter()
            .chain(&c.methods)
            .chain(&c.static_methods)
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
        {
            stmts(&f.body, module, prefix, class_reps, &mut out);
        }
        for f in c.fields.iter().chain(&c.static_fields) {
            if let Some(e) = &f.init {
                expr(e, module, prefix, class_reps, &mut out);
            }
        }
    }
    out.into_iter()
        .map(|shape| ModuleBirth {
            keys_global: format!("perry_constfn_final_{}", shape.constfn[0].symbol),
            class_id: 0,
            defined: false,
            shape,
        })
        .collect()
}

pub(crate) fn entries_symbol(prefix: &str, id: u32) -> String {
    format!("perry_constfn_final_{prefix}__{id}")
}

pub(crate) fn emit_final_entries(
    module: &mut crate::module::LlModule,
    prefix: &str,
    shapes: &[(BirthShape, u32)],
) {
    for (shape, id) in shapes.iter().filter(|(shape, _)| !shape.constfn.is_empty()) {
        let entries = shape
            .constfn
            .iter()
            .map(|e| {
                module.request_static_seed_body(
                    e.symbol.strip_suffix("$info").expect("body info suffix"),
                );
                format!("{{ i32, ptr }} {{ i32 {}, ptr @{} }}", e.slot, e.symbol)
            })
            .collect::<Vec<_>>()
            .join(", ");
        module.add_raw_global(format!(
            "@{} = private constant [{} x {{ i32, ptr }}] [{entries}]",
            entries_symbol(prefix, *id),
            shape.constfn.len()
        ));
    }
}

pub(crate) fn has_final_shapes() -> bool {
    super::static_shape_ids::has_static_final_shapes()
}

/// Called only below the last store/this patch. The runtime owns a root during
/// minting and returns its refreshed handle for the expression's result.
pub(crate) fn finalize_literal(
    ctx: &mut crate::expr::FnCtx<'_>,
    props: &[(String, Expr)],
    base_rep: u64,
    object: &str,
) -> String {
    if !super::static_shape_ids::has_static_final_shapes() {
        return object.to_string();
    }
    let Some(shape) = literal_final(ctx.strings.module_prefix(), props, base_rep) else {
        return object.to_string();
    };
    finalize_shape(ctx, &shape, object)
}

pub(crate) fn finalize_class(ctx: &mut crate::expr::FnCtx<'_>, name: &str, boxed: &str) -> String {
    let shape = ctx.classes.get(name).and_then(|class| {
        let rep = *ctx
            .class_birth_reps
            .get(ctx.class_keys_globals.get(name)?)?;
        let cid = *ctx.class_ids.get(name)?;
        super::static_constfn_class::class_final(
            ctx.strings.module_prefix(),
            class,
            ctx.classes,
            rep,
            cid,
        )
        .ok()
    });
    let Some(shape) = shape else {
        return boxed.to_string();
    };
    if super::static_shape_ids::static_final_shape_id(&shape).is_none() {
        return boxed.to_string();
    }
    // `boxed` is the completed receiver, including constructor return override.
    // The proof declines replacement-return constructors. Never recover the
    // original allocation root here: super()/constructors own this selection.
    let bits = ctx.block().bitcast_double_to_i64(boxed);
    let handle = ctx
        .block()
        .and(crate::types::I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let result = finalize_shape(ctx, &shape, &handle);
    crate::expr::nanbox_pointer_inline(ctx.block(), &result)
}

fn finalize_shape(ctx: &mut crate::expr::FnCtx<'_>, shape: &BirthShape, object: &str) -> String {
    let Some(id) = super::static_shape_ids::static_final_shape_id(&shape) else {
        return object.to_string();
    };
    let entries = format!("@{}", entries_symbol(ctx.strings.module_prefix(), id));
    let packed = String::from_utf8(shape.keys.clone()).expect("UTF-8 property names");
    let key_idx = ctx.strings.intern(&packed);
    let key = ctx.strings.entry(key_idx);
    let global = format!("@{}", key.bytes_global);
    let len = key.byte_len.to_string();
    use crate::types::{I32, I64, PTR};
    ctx.block().call(
        I64,
        "js_object_finalize_constfn_static",
        &[
            (I64, object),
            (I32, &id.to_string()),
            (PTR, &global),
            (I32, &len),
            (I32, &shape.key_count.to_string()),
            (I32, &shape.live.to_string()),
            (
                I32,
                &match shape.proto {
                    BirthProto::Literal => 0,
                    BirthProto::Class(cid) => cid,
                }
                .to_string(),
            ),
            (I64, &shape.rep.to_string()),
            (PTR, &entries),
            (I32, &shape.constfn.len().to_string()),
        ],
    )
}

#[cfg(test)]
#[path = "static_constfn_tests.rs"]
mod tests;
