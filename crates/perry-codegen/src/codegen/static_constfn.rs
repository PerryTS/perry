//! Static ConstFn describes a completed object. Allocation ids remain Any/F64.
use super::{BirthProto, BirthShape, ConstFnBirth, ModuleBirth};
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{BTreeSet, HashMap};

/// Executables build ConstFn lanes unless `PERRY_CONSTFN_SHAPE=0`; a dylib
/// never does.
pub(crate) fn enabled(output_type: &str) -> bool {
    output_type == "executable" && std::env::var("PERRY_CONSTFN_SHAPE").as_deref() != Ok("0")
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
        } else if let Expr::FuncRef(func_id) = value {
            // A method shorthand that never reads `this` is hoisted to a
            // module function and stored as that function's value: its
            // singleton closure, whose body is the `__perry_wrap_` JS-ABI
            // wrapper. That body is as permanent as a closure body.
            let Some((body, _)) = module_function_value_body(*func_id) else {
                continue;
            };
            if (base_rep >> (slot * 2)) & 3 != 0 {
                return None;
            }
            rep |= 3 << (slot * 2);
            constfn.push(ConstFnBirth {
                slot: slot as u8,
                symbol: crate::fn_info::info_symbol(&body),
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
        private: Vec::new(),
        brands: Vec::new(),
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
    class_widths: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    fn expr(
        e: &Expr,
        module: &Module,
        prefix: &str,
        reps: &HashMap<String, u64>,
        widths: &HashMap<String, u32>,
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
                .and_then(|props| literal_final(prefix, &props, *reps.get(class_name)?))
                .map(|mut shape| {
                    shape.live = widths.get(class_name).copied().unwrap_or(shape.live);
                    shape
                }),
            _ => None,
        };
        if let Some(shape) = shape {
            out.insert(shape);
        }
        if let Expr::Closure { body, .. } = e {
            stmts(body, module, prefix, reps, widths, out);
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| {
            expr(child, module, prefix, reps, widths, out)
        });
    }
    fn stmts(
        body: &[Stmt],
        m: &Module,
        p: &str,
        r: &HashMap<String, u64>,
        w: &HashMap<String, u32>,
        out: &mut BTreeSet<BirthShape>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Let { init, .. } | Stmt::Return(init) => {
                    if let Some(e) = init {
                        expr(e, m, p, r, w, out);
                    }
                }
                Stmt::Expr(e) | Stmt::Throw(e) => expr(e, m, p, r, w, out),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    expr(condition, m, p, r, w, out);
                    stmts(then_branch, m, p, r, w, out);
                    if let Some(b) = else_branch {
                        stmts(b, m, p, r, w, out);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                    expr(condition, m, p, r, w, out);
                    stmts(body, m, p, r, w, out);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(s) = init {
                        stmts(std::slice::from_ref(s), m, p, r, w, out);
                    }
                    for e in [condition, update].into_iter().flatten() {
                        expr(e, m, p, r, w, out);
                    }
                    stmts(body, m, p, r, w, out);
                }
                Stmt::Labeled { body, .. } => stmts(std::slice::from_ref(body), m, p, r, w, out),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    stmts(body, m, p, r, w, out);
                    if let Some(c) = catch {
                        stmts(&c.body, m, p, r, w, out);
                    }
                    if let Some(b) = finally {
                        stmts(b, m, p, r, w, out);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    expr(discriminant, m, p, r, w, out);
                    for c in cases {
                        if let Some(e) = &c.test {
                            expr(e, m, p, r, w, out);
                        }
                        stmts(&c.body, m, p, r, w, out);
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
    stmts(
        &module.init,
        module,
        prefix,
        class_reps,
        class_widths,
        &mut out,
    );
    for global in &module.globals {
        if let Some(e) = &global.init {
            expr(e, module, prefix, class_reps, class_widths, &mut out);
        }
    }
    for f in &module.functions {
        stmts(&f.body, module, prefix, class_reps, class_widths, &mut out);
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
            stmts(&f.body, module, prefix, class_reps, class_widths, &mut out);
        }
        for f in c.fields.iter().chain(&c.static_fields) {
            if let Some(e) = &f.init {
                expr(e, module, prefix, class_reps, class_widths, &mut out);
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
        // Packed key names are finalizer metadata, not JavaScript strings.
        // Putting them in StringPool would allocate and root a heap string
        // for every distinct final layout, although only its bytes are used.
        let packed = shape
            .keys
            .iter()
            .map(|byte| format!("\\{byte:02X}"))
            .collect::<String>();
        module.add_named_string_constant(
            &format!("{}_keys", entries_symbol(prefix, *id)),
            shape.keys.len(),
            &format!("c\"{packed}\""),
        );
    }
}

pub(crate) fn has_final_shapes() -> bool {
    super::static_shape_ids::has_static_final_shapes()
}

thread_local! {
    /// Parameter count of every non-rest closure body of the module being
    /// compiled, by body symbol. A static method lane calls its body directly,
    /// and a direct call must match the definition's signature exactly.
    static MODULE_BODY_ARITIES: std::cell::RefCell<HashMap<String, usize>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Install the current module's body arities (from closure collection, and
/// the function-value wrappers [`set_module_function_values`] admitted).
pub(crate) fn set_module_body_arities(prefix: &str, closure_arities: &HashMap<u32, u32>) {
    let mut map: HashMap<String, usize> = closure_arities
        .iter()
        .map(|(&fid, &arity)| {
            (
                crate::fn_info::closure_body_symbol(prefix, fid),
                arity as usize,
            )
        })
        .collect();
    MODULE_FUNCTION_VALUES.with(|m| {
        map.extend(m.borrow().values().cloned());
    });
    MODULE_BODY_ARITIES.with(|m| *m.borrow_mut() = map);
}

thread_local! {
    /// The module's functions whose VALUE (`Expr::FuncRef`) a ConstFn lane may
    /// name: function id -> (wrapper body symbol, declared parameter count).
    static MODULE_FUNCTION_VALUES: std::cell::RefCell<HashMap<u32, (String, usize)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Install the module's admissible function values before any birth or
/// lowering reads them. A function qualifies when its value's body (the
/// `__perry_wrap_` wrapper, JS body ABI, one double per declared parameter)
/// can be entered directly: not async, not a generator, no rest parameter and
/// no `arguments` object. Symbol names come from the same registry the
/// wrapper emission uses.
pub(crate) fn set_module_function_values(module: &Module, prefix: &str) {
    let names = super::func_registry::build_func_registry(module, prefix).func_names;
    let map = module
        .functions
        .iter()
        .filter(|f| {
            !f.is_async
                && !f.is_generator
                && !f
                    .params
                    .iter()
                    .any(|p| p.is_rest || p.arguments_object.is_some())
        })
        .filter_map(|f| {
            let name = names.get(&f.id)?;
            Some((f.id, (format!("__perry_wrap_{name}"), f.params.len())))
        })
        .collect();
    MODULE_FUNCTION_VALUES.with(|m| *m.borrow_mut() = map);
}

thread_local! {
    /// The module's candidate prototype classes of `Object.create` values
    /// (`collectors::object_create_protos`): candidates, never proofs.
    static MODULE_OBJECT_CREATE_PROTOS: std::cell::RefCell<crate::collectors::ObjectCreateProtos> =
        std::cell::RefCell::new(Default::default());
}

/// Install the module's `Object.create` candidates before lowering reads them.
pub(crate) fn set_module_object_create_protos(module: &Module) {
    let protos = crate::collectors::object_create_protos(module);
    MODULE_OBJECT_CREATE_PROTOS.with(|m| *m.borrow_mut() = protos);
}

fn module_function_value_body(func_id: u32) -> Option<(String, usize)> {
    MODULE_FUNCTION_VALUES.with(|m| m.borrow().get(&func_id).cloned())
}

fn module_body_arity(body: &str) -> Option<usize> {
    MODULE_BODY_ARITIES.with(|m| m.borrow().get(body).copied())
}

/// The most completed shapes one site compares before its learned memo.
const MAX_STATIC_METHOD_LANES: usize = 2;

/// One completed shape whose ConstFn lane answers `recv.key(...)`: a receiver
/// whose header word equals `word` holds, in inline slot `slot`, a closure of
/// `body` (the shape's invariant), so the site calls `body` directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StaticMethodLane {
    /// `(ShapeId << 32) | class_id`, the receiver's first header word.
    pub word: u64,
    pub slot: u32,
    /// The body's symbol (JS body ABI), defined in this module.
    pub body: String,
    /// The body's declared parameter count.
    pub arity: usize,
    /// The receiver may itself carry `word`, so the site compares it. False
    /// for a value made by `Object.create(P)`: it inherits the method, so the
    /// lanes of P's class only name candidate bodies for the learned
    /// inherited hit.
    pub own: bool,
}

/// The completed shapes a method site may compare for `object.property(...)`.
///
/// The receiver's candidate class is the one the guarded field sites already
/// use: a proven class, the local's declared/inferred hint, or the guarded
/// `this` of a closed-shape literal method. A candidate is only a guess the
/// emitted shape compare checks, so any candidate is sound; only a
/// literal-shape class has completed ConstFn shapes. Its birth id's compatible
/// completed shapes (the ids the body guards also accept) that carry a ConstFn
/// lane at `property`'s slot give the lanes.
pub(crate) fn static_method_lanes(
    ctx: &crate::expr::FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Vec<StaticMethodLane> {
    if property.is_empty() || !has_final_shapes() {
        return Vec::new();
    }
    let named = |t: Option<&perry_hir::types::Type>| match t? {
        perry_hir::types::Type::Named(name) => Some(name.clone()),
        _ => None,
    };
    let candidate =
        crate::type_analysis::receiver_class_name(ctx, object).or_else(|| match object {
            Expr::LocalGet(id) => named(ctx.local_type_hint(id)),
            Expr::This => ctx.guarded_this_class.clone(),
            _ => None,
        });
    // A value made by `Object.create(P)` (directly, through a const binding or
    // a factory's return) inherits from P: P's literal class is its candidate.
    let (class_name, own) = match candidate {
        Some(name) => (name, true),
        None => match MODULE_OBJECT_CREATE_PROTOS.with(|m| m.borrow().of(object).cloned()) {
            Some(name) => (name, false),
            None => return Vec::new(),
        },
    };
    let Some(class) = ctx.classes.get(&class_name) else {
        return Vec::new();
    };
    if !class.is_literal_shape() {
        return Vec::new();
    }
    let (Some(&class_id), Some(keys_global)) = (
        ctx.class_ids.get(&class_name),
        ctx.class_keys_globals.get(&class_name),
    ) else {
        return Vec::new();
    };
    let Some(birth) = super::static_shape_ids::static_shape_id_for_keys_global(keys_global) else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    for (id, shape) in super::compatible_final_shapes(&birth.to_string(), &[]) {
        let Some(slot) = shape
            .keys
            .split(|&b| b == 0)
            .take(shape.key_count as usize)
            .position(|k| k == property.as_bytes())
        else {
            continue;
        };
        let Some(entry) = shape.constfn.iter().find(|e| e.slot as usize == slot) else {
            continue;
        };
        let Some(body) = entry.symbol.strip_suffix("$info") else {
            continue;
        };
        let Some(arity) = module_body_arity(body) else {
            continue;
        };
        lanes.push(StaticMethodLane {
            word: (u64::from(id) << 32) | u64::from(class_id),
            slot: slot as u32,
            body: body.to_string(),
            arity,
            own,
        });
    }
    if lanes.len() > MAX_STATIC_METHOD_LANES {
        lanes.clear();
    }
    lanes
}

/// Called only below the last store/this patch. The runtime owns a root during
/// minting and returns its refreshed handle for the expression's result.
pub(crate) fn finalize_literal(
    ctx: &mut crate::expr::FnCtx<'_>,
    props: &[(String, Expr)],
    base_rep: u64,
    object: &str,
    live: u32,
) -> String {
    if !super::static_shape_ids::has_static_final_shapes() {
        return object.to_string();
    }
    let Some(mut shape) = literal_final(ctx.strings.module_prefix(), props, base_rep) else {
        return object.to_string();
    };
    // Finalization validates the complete live bound. Keep the allocation
    // capacity so widening a method literal does not retire its ConstFn lanes.
    shape.live = live;
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
    let global = format!("{entries}_keys");
    let len = shape.keys.len().to_string();
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
