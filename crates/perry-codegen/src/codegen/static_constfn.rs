//! Static ConstFn shapes describe completed objects and literal births.
use super::{BirthProto, BirthShape, ConstFnBirth, ModuleBirth};
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{BTreeSet, HashMap};

/// Executables build ConstFn lanes unless the compile's options disable them
/// (`CompileOptions::disable_constfn_shapes`); a dylib never does. Only the
/// options decide: a process-wide input read here would let one compile's
/// choice leak into another running in parallel.
pub(crate) fn enabled(opts: &super::CompileOptions) -> bool {
    opts.output_type == "executable" && !opts.disable_constfn_shapes
}

pub(crate) fn literal_final(
    prefix: &str,
    props: &[(String, Expr)],
    base_rep: u64,
) -> Option<BirthShape> {
    if props.is_empty() {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    // A numeric annotation is not a birth proof. Keep a raw lane only
    // for a numeric literal; all other values retain their NaN-boxed bits.
    let mut rep = base_rep;
    for (slot, (_, value)) in props.iter().enumerate() {
        if slot < 32
            && (rep >> (slot * 2)) & 3 == 1
            && !matches!(value, Expr::Number(_) | Expr::Integer(_))
        {
            rep &= !(3 << (slot * 2));
        }
    }
    let mut constfn = Vec::new();
    for (slot, (key, value)) in props.iter().enumerate() {
        if key.is_empty() || key.contains('\0') || key == "__proto__" || !seen.insert(key) {
            return None;
        }
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        if slot >= 32 {
            continue;
        }
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
        attrs: Vec::new(),
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

pub(crate) fn collect_literal_finals(
    module: &Module,
    prefix: &str,
    class_reps: &HashMap<String, u64>,
    class_widths: &HashMap<String, u32>,
) -> LiteralFinals {
    fn expr(
        e: &Expr,
        module: &Module,
        prefix: &str,
        reps: &HashMap<String, u64>,
        widths: &HashMap<String, u32>,
        out: &mut LiteralFinals,
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
            out.shapes.insert(shape);
        }
        if let Expr::Closure { body, .. } = e {
            stmts(body, module, prefix, reps, widths, out);
        }
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::Closure { params, body, .. } = callee.as_ref() {
                // An IIFE binds its parameters to its arguments and runs its
                // body once: walk that body once, with the bindings, then the
                // arguments and parameter defaults. Visiting the callee as a
                // child would walk the body (and the arguments) a second
                // time, which doubles per nesting level.
                let seeds: Vec<(u32, &Expr)> = params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.id, arg))
                    .collect();
                block(&seeds, body, module, prefix, reps, widths, out);
                for default in params.iter().filter_map(|param| param.default.as_ref()) {
                    expr(default, module, prefix, reps, widths, out);
                }
                for arg in args {
                    expr(arg, module, prefix, reps, widths, out);
                }
                return;
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| {
            expr(child, module, prefix, reps, widths, out)
        });
    }
    type Builders = HashMap<u32, (Vec<(String, Expr)>, u32, u32, bool)>;
    fn stmts(
        body: &[Stmt],
        m: &Module,
        p: &str,
        r: &HashMap<String, u64>,
        w: &HashMap<String, u32>,
        out: &mut LiteralFinals,
    ) {
        block(&[], body, m, p, r, w, out);
    }
    /// One local bound to its initializer: a `let`, or an IIFE parameter
    /// bound to its argument.
    fn bind(id: u32, seed: &Expr, m: &Module, builders: &mut Builders, out: &mut LiteralFinals) {
        let props = match seed {
            Expr::Object(props) => Some(props.clone()),
            Expr::New {
                class_name, args, ..
            } => m
                .classes
                .iter()
                .find(|c| c.name == *class_name)
                .and_then(|c| anon_props(c, args)),
            Expr::PropertyGet {
                object, property, ..
            } => match object.as_ref() {
                Expr::LocalGet(owner) => builders
                    .get(owner)
                    .and_then(|(props, _, _, _)| props.iter().find(|(key, _)| key == property))
                    .and_then(|(_, value)| match value {
                        Expr::Object(props) => Some(props.clone()),
                        Expr::New {
                            class_name, args, ..
                        } => m
                            .classes
                            .iter()
                            .find(|c| c.name == *class_name)
                            .and_then(|c| anon_props(c, args)),
                        _ => None,
                    }),
                _ => None,
            },
            _ => None,
        };
        if let Some(source) = ObjectSource::of(seed) {
            out.sources.insert(id, source);
        }
        let returned = match seed {
            Expr::Closure { body, .. } => ObjectSource::returned(body),
            Expr::FuncRef(fid) => m
                .functions
                .iter()
                .find(|f| f.id == *fid)
                .and_then(|f| ObjectSource::returned(&f.body)),
            _ => None,
        };
        if let Some(source) = returned {
            out.callables.insert(id, source);
        }
        if let Expr::PropertyGet {
            object, property, ..
        } = seed
        {
            if let Expr::LocalGet(owner) = object.as_ref() {
                out.properties.insert((*owner, property.clone()), id);
            }
        }
        if let Some(props) = props {
            let capacity = (props.len() as u32).max(crate::target_layout::INLINE_SLOT_FLOOR as u32);
            let birth_keys = props.len() as u32;
            builders.insert(id, (props, capacity, birth_keys, false));
        }
    }
    /// `body` with `seeds` bound first (an IIFE's parameters). Seed values
    /// are bound, not walked: the caller walks them as the call's arguments.
    fn block(
        seeds: &[(u32, &Expr)],
        body: &[Stmt],
        m: &Module,
        p: &str,
        r: &HashMap<String, u64>,
        w: &HashMap<String, u32>,
        out: &mut LiteralFinals,
    ) {
        // Follow the very same source-ordered construction as literal births.
        // These are candidate contents, not a proof that no code adds a key.
        // The call's full layout comparison is the authority.
        let mut builders: Builders = HashMap::new();
        for &(id, seed) in seeds {
            bind(id, seed, m, &mut builders, out);
        }
        for stmt in body {
            if let Stmt::Let {
                id,
                init: Some(seed),
                ..
            } = stmt
            {
                bind(*id, seed, m, &mut builders, out);
            }
            if let Stmt::Expr(e) = stmt {
                let store = match e {
                    Expr::PropertySet {
                        object,
                        property,
                        value,
                    } => Some((object.as_ref(), property.as_str(), value.as_ref())),
                    Expr::IndexSet {
                        object,
                        index,
                        value,
                    } => match index.as_ref() {
                        Expr::String(key) => Some((object.as_ref(), key.as_str(), value.as_ref())),
                        _ => None,
                    },
                    Expr::PutValueSet {
                        target, key, value, ..
                    } => match key.as_ref() {
                        Expr::String(key) => Some((target.as_ref(), key.as_str(), value.as_ref())),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((Expr::ExternFuncRef { name, .. }, key, _)) = store {
                    if !key.is_empty() && !key.contains('\0') && key != "__proto__" {
                        out.imported_writes.push((name.clone(), key.to_string()));
                    }
                }
                if let Some((Expr::LocalGet(id), key, value)) = store {
                    if let Some((props, _, _, changed)) = builders.get_mut(id) {
                        *changed = true;
                        if let Some((_, old)) = props.iter_mut().find(|(name, _)| name == key) {
                            // A declared candidate survives reassignment. The
                            // runtime compares the selected value with this
                            // body; later unknown/different values miss safely.
                            if !matches!(old, Expr::FuncRef(_) | Expr::Closure { .. }) {
                                *old = value.clone();
                            }
                        } else {
                            props.push((key.into(), value.clone()));
                        }
                    }
                }
            }
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
        for (id, (props, capacity, birth_keys, changed)) in builders {
            if !changed || props.len() as u32 == birth_keys {
                continue;
            }
            if let Some(object) = construction_final(m, p, &props, capacity, birth_keys) {
                out.shapes.insert(object.shape.clone());
                out.objects.insert(id, object);
            }
        }
    }
    let mut out = LiteralFinals::default();
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
    // Outlined module init and CJS/IIFE return bindings share the same
    // source object. Resolve their compile-time provenance after visiting
    // every body; the eventual call still proves the current value/layout.
    let bindings: Vec<u32> = out.sources.keys().copied().collect();
    for binding in bindings {
        if let Some(object) =
            resolve_object_source(&ObjectSource::Binding(binding), &out, module, 0)
        {
            out.objects.insert(binding, object);
        }
    }
    out
}

#[derive(Default)]
pub(crate) struct LiteralFinals {
    pub shapes: BTreeSet<BirthShape>,
    pub objects: HashMap<u32, ConstructedObject>,
    pub imported_writes: Vec<(String, String)>,
    sources: HashMap<u32, ObjectSource>,
    callables: HashMap<u32, ObjectSource>,
    properties: HashMap<(u32, String), u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct ConstructedObject {
    pub shape: BirthShape,
    pub methods: Vec<super::ImportedObjectLiteralMethod>,
}

/// Scalar construction provenance collected by the same literal visitor.
/// This nominates a layout/callee, never grants a proof to emitted code.
#[derive(Clone, Debug)]
enum ObjectSource {
    Binding(u32),
    Property(u32, String),
    CallLocal(u32),
    CallFunction(u32),
}
impl ObjectSource {
    fn of(expr: &Expr) -> Option<Self> {
        match expr {
            Expr::LocalGet(id) => Some(Self::Binding(*id)),
            Expr::PropertyGet {
                object, property, ..
            } => match object.as_ref() {
                Expr::LocalGet(id) => Some(Self::Property(*id, property.clone())),
                _ => None,
            },
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::LocalGet(id) => Some(Self::CallLocal(*id)),
                Expr::FuncRef(id) => Some(Self::CallFunction(*id)),
                Expr::Closure { body, .. } => Self::returned(body),
                _ => None,
            },
            _ => None,
        }
    }
    fn returned(body: &[Stmt]) -> Option<Self> {
        match body.last()? {
            Stmt::Return(Some(value)) => Self::of(value),
            _ => None,
        }
    }
}
fn resolve_object_source(
    source: &ObjectSource,
    finals: &LiteralFinals,
    module: &Module,
    depth: u32,
) -> Option<ConstructedObject> {
    if depth >= 32 {
        return None;
    }
    match source {
        ObjectSource::Binding(id) => {
            finals.objects.get(id).cloned().or_else(|| {
                resolve_object_source(finals.sources.get(id)?, finals, module, depth + 1)
            })
        }
        ObjectSource::Property(id, key) => resolve_object_source(
            &ObjectSource::Binding(*finals.properties.get(&(*id, key.clone()))?),
            finals,
            module,
            depth + 1,
        ),
        ObjectSource::CallLocal(id) => {
            resolve_object_source(finals.callables.get(id)?, finals, module, depth + 1)
        }
        ObjectSource::CallFunction(id) => {
            let function = module.functions.iter().find(|f| f.id == *id)?;
            resolve_object_source(
                &ObjectSource::returned(&function.body)?,
                finals,
                module,
                depth + 1,
            )
        }
    }
}

/// Nominate a completed literal-builder content. Only representation in the
/// physical inline prefix is described; the remaining values are ordinary
/// boxed spill slots. A subsequent unknown write simply makes the guard miss.
fn construction_final(
    module: &Module,
    prefix: &str,
    props: &[(String, Expr)],
    capacity: u32,
    birth_keys: u32,
) -> Option<ConstructedObject> {
    if props.is_empty()
        || !props
            .iter()
            .any(|(_, v)| matches!(v, Expr::FuncRef(_) | Expr::Closure { .. }))
    {
        return None;
    }
    let names = super::func_registry::build_func_registry(module, prefix).func_names;
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    let mut rep = 0;
    let mut methods = Vec::new();
    for (slot, (key, value)) in props.iter().enumerate() {
        if key.is_empty() || key.contains('\0') || key == "__proto__" || !seen.insert(key) {
            return None;
        }
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        if slot >= birth_keys as usize
            && slot < capacity as usize
            && slot < 32
            && matches!(
                value,
                Expr::Number(_) | Expr::Integer(_) | Expr::EnumMember { .. }
            )
        {
            rep |= 1u64 << (2 * slot);
        }
        // Module function references have one canonical, capture-free value.
        // This is the same wrapper eligibility as literal ConstFn births.
        if let Expr::FuncRef(id) = value {
            let Some(function) = module.functions.iter().find(|f| f.id == *id) else {
                continue;
            };
            if function.is_async
                || function.is_generator
                || function
                    .params
                    .iter()
                    .any(|p| p.is_rest || p.arguments_object.is_some())
            {
                continue;
            }
            let Some(name) = names.get(id) else {
                continue;
            };
            methods.push(super::ImportedObjectLiteralMethod {
                name: key.clone(),
                func_id: *id,
                target: format!("__perry_wrap_{name}"),
                param_count: function.params.len(),
                field_index: slot as u32,
            });
        } else if let Expr::Closure {
            func_id,
            params,
            captures_this,
            is_arrow,
            is_async: false,
            is_generator: false,
            ..
        } = value
        {
            if (*captures_this && !*is_arrow)
                || params
                    .iter()
                    .any(|p| p.is_rest || p.arguments_object.is_some())
            {
                continue;
            }
            methods.push(super::ImportedObjectLiteralMethod {
                name: key.clone(),
                func_id: *func_id,
                target: crate::fn_info::closure_body_symbol(prefix, *func_id),
                param_count: params.len(),
                field_index: slot as u32,
            });
        }
    }
    if methods.is_empty() {
        return None;
    }
    let live = capacity.min(props.len() as u32);
    // Key adds normalize prior SPECIAL lanes. Only a final inline function
    // add can leave a ConstFn lane; spilled function values remain Any and
    // require the call site's independent callee identity comparison.
    let mut constfn = Vec::new();
    if let Some(last) = methods.iter().find(|m| {
        m.field_index as usize + 1 == props.len() && m.field_index < live && m.field_index < 32
    }) {
        rep |= 3u64 << (2 * last.field_index);
        constfn.push(ConstFnBirth {
            slot: last.field_index as u8,
            symbol: crate::fn_info::info_symbol(&last.target),
        });
    }
    let shape = BirthShape {
        keys,
        key_count: props.len() as u32,
        live,
        proto: BirthProto::Literal,
        typed: None,
        rep,
        constfn,
        private: Vec::new(),
        brands: Vec::new(),
        attrs: Vec::new(),
    };
    Some(ConstructedObject { shape, methods })
}

pub(crate) fn module_literal_finals(
    module: &Module,
    prefix: &str,
    reps: &HashMap<String, u64>,
    widths: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    collect_literal_finals(module, prefix, reps, widths)
        .shapes
        .into_iter()
        .enumerate()
        .map(|(i, shape)| ModuleBirth {
            keys_global: format!("perry_literal_final_{prefix}_{i}"),
            class_id: 0,
            defined: false,
            shape,
        })
        .collect()
}

thread_local! {
    static MODULE_CONSTRUCTIONS: std::cell::RefCell<HashMap<u32, ConstructedObject>> = Default::default();
}

pub(crate) fn set_module_constructions(module: &Module, prefix: &str) {
    let objects = collect_literal_finals(module, prefix, &HashMap::new(), &HashMap::new()).objects;
    MODULE_CONSTRUCTIONS.with(|m| *m.borrow_mut() = objects);
}

pub(crate) fn constructed_object(
    ctx: &crate::expr::FnCtx<'_>,
    object: &Expr,
) -> Option<ConstructedObject> {
    match object {
        Expr::LocalGet(id) => MODULE_CONSTRUCTIONS
            .with(|m| m.borrow().get(id).cloned())
            .or_else(|| {
                let binding = ctx.local_imported_object_aliases.get(id)?;
                let imported = ctx.imported_object_literals.get(binding)?;
                Some(ConstructedObject {
                    shape: imported.final_shape.clone()?,
                    methods: imported.methods.clone(),
                })
            }),
        Expr::ExternFuncRef { name, .. } => {
            let imported = ctx.imported_object_literals.get(name)?;
            Some(ConstructedObject {
                shape: imported.final_shape.clone()?,
                methods: imported.methods.clone(),
            })
        }
        _ => None,
    }
}

pub(crate) fn literal_seed_descriptors() -> Vec<(BirthShape, u32)> {
    super::static_shape_ids::module_final_seeds()
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
        return super::static_prototype::inherited_lane(ctx, &class_name, property)
            .into_iter()
            .collect();
    }
    let (Some(&class_id), Some(keys_global)) = (
        ctx.class_ids.get(&class_name),
        ctx.class_keys_globals.get(&class_name),
    ) else {
        return Vec::new();
    };
    let Some(birth) = super::static_shape_ids::static_layout_word_for_keys_global(keys_global)
    else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    for (id, shape) in super::compatible_final_layouts(&birth.to_string(), &[]) {
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

/// Literal values are evaluated and rooted before their single completed
/// birth. Shape metadata is consumed at the seed, never rebuilt per evaluation.
pub(crate) fn birth_literal(
    ctx: &mut crate::expr::FnCtx<'_>,
    props: &[(String, Expr)],
    base_rep: u64,
    live: u32,
    class_id: u32,
) -> Option<anyhow::Result<String>> {
    if !super::static_shape_ids::has_static_final_shapes()
        || !crate::layout_desc::layouts_enabled("executable", ctx.target_triple)
    {
        return None;
    }
    let mut shape = literal_final(ctx.strings.module_prefix(), props, base_rep)?;
    shape.live = live;
    let id = super::static_shape_ids::static_final_shape_identity(&shape)?;
    let desc = format!(
        "@{}",
        crate::layout_desc::literal_desc_symbol(ctx.strings.module_prefix(), id)
    );
    let patches: Vec<_> = props
        .iter()
        .enumerate()
        .filter_map(|(slot, (_, expr))| {
            if let Expr::Closure {
                params,
                body,
                captures,
                captures_this: true,
                is_arrow: false,
                ..
            } = expr
            {
                Some((
                    slot,
                    crate::type_analysis::compute_auto_captures(ctx, params, body, captures).len(),
                ))
            } else {
                None
            }
        })
        .collect();
    let exprs: Vec<_> = props.iter().map(|(_, expr)| expr).collect();
    Some(crate::rooting::with_operands_rooted_across_call(
        ctx,
        &exprs,
        |ctx| Ok(crate::expr::inline_birth::layout(ctx, &desc, shape.live, 0)),
        |ctx, values, object| {
            use crate::types::{I32, I64, I8};
            let raw = ctx.block().inttoptr(I64, &object);
            // The literal's synthetic class identity is independent of its layout.
            // GC_STORE_AUDIT(POINTER_FREE): The i32 store writes the synthetic class ID at ObjectHeader offset0, not a pointer or JSValue; rooted operands survive the surrounding inline_birth allocation.
            ctx.block().store(I32, &class_id.to_string(), &raw);
            let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
            let fields = ctx.block().gep(I8, &raw, &[(I64, &header.to_string())]);
            for (slot, value) in values.iter().enumerate() {
                // A chunked literal slot (`Expr::LiteralChunks`) stays a hole
                // of the unpublished birth; its chunk function stores it.
                if matches!(props[slot].1, Expr::LiteralChunks(_)) {
                    continue;
                }
                let bits = if slot < 32 && (shape.rep >> (slot * 2)) & 3 == 1 {
                    // Numeric literal births may lower to the int32 NaN-box.
                    // The compiler owns this conversion; no runtime admission.
                    let number = match &props[slot].1 {
                        Expr::Number(n) => crate::nanbox::double_literal(*n),
                        Expr::Integer(n) => crate::nanbox::double_literal(*n as f64),
                        _ => unreachable!("raw birth lane is proven numeric"),
                    };
                    ctx.block().bitcast_double_to_i64(&number)
                } else {
                    ctx.block().bitcast_double_to_i64(value)
                };
                let ptr = ctx.block().gep(I64, &fields, &[(I64, &slot.to_string())]);
                // GC_STORE_AUDIT(INIT): fixed slot of an unpublished layout birth;
                // its barrier is the birth's one barrier below.
                ctx.block().store(I64, &bits, &ptr);
            }
            // The stores above are plain stores into the one fresh object, with
            // no call or safepoint among them: one parent gate decides every
            // slot's barrier, and the runtime replays them (raw numeric lanes
            // carry doubles, not values, and owe none).
            let raw = (0..props.len().min(32))
                .filter(|&slot| (shape.rep >> (slot * 2)) & 3 == 1)
                .fold(0u32, |raw, slot| raw | 1 << slot);
            let address = ctx.block().ptrtoint(&fields, I64);
            crate::expr::emit_write_barrier_birth_slots_generation_tested(
                ctx,
                &object,
                &address,
                props.len(),
                raw,
                "literal_birth",
            );
            // Reserved slots are absent until written.
            // The born object must survive every patch helper and be re-read
            // below it. The operand scope still owns all evaluated values.
            let mut group = crate::rooting::open_rooted_group(1);
            let owner =
                group.adopt_emitted(ctx, crate::rooting::Repr::Ptr, &object, !patches.is_empty());
            for (slot, capture) in &patches {
                let current = group.reread_emitted(ctx, owner);
                let bits = ctx.block().call(
                    I64,
                    "js_object_get_field",
                    &[(I64, &current), (I32, &slot.to_string())],
                );
                let closure = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
                let current = group.reread_emitted(ctx, owner);
                let boxed_bits = ctx
                    .block()
                    .or(I64, &current, crate::nanbox::POINTER_TAG_I64);
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[
                        (I64, &closure),
                        (I32, &capture.to_string()),
                        (I64, &boxed_bits),
                    ],
                );
            }
            let current = group.reread_emitted(ctx, owner);
            let boxed = crate::expr::nanbox_pointer_inline(ctx.block(), &current);
            group.release(ctx);
            Ok(boxed)
        },
    ))
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
    if super::static_shape_ids::static_final_shape_identity(&shape).is_none() {
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
    let Some(id) = super::static_shape_ids::static_final_shape_identity(&shape) else {
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
            (I32, &crate::layout_ordinals::static_word(id).to_string()),
            (PTR, &global),
            (I32, &len),
            (I32, &shape.key_count.to_string()),
            (I32, &shape.live.to_string()),
            (
                I32,
                &match shape.proto {
                    BirthProto::Literal => 0,
                    BirthProto::Class(cid) => cid,
                    BirthProto::Prototype(..) => {
                        unreachable!("prototype uses lazy materialization")
                    }
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
