use crate::types::{LocalId, Type};
use anyhow::{anyhow, Result};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

pub fn lower_constructor(
    ctx: &mut LoweringContext,
    class_name: &str,
    ctor: &ast::Constructor,
) -> Result<Function> {
    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    ctx.enter_strict_mode(true);

    // Track that we're inside a constructor body so `new.target` can resolve
    // to a placeholder object with `.name = class_name`. Saved/restored in
    // case constructors are nested via class expressions.
    let saved_ctor_class = ctx.in_constructor_class.take();
    ctx.in_constructor_class = Some(class_name.to_string());

    // Add 'this' as a special local
    let _this_id = ctx.define_local("this".to_string(), Type::Any);

    // Lower parameters with type extraction (using context for class type param resolution)
    let mut params = Vec::new();
    // Track TsParamProp params so we can synthesize `this.field = param` assignments
    let mut param_prop_assignments: Vec<(LocalId, String)> = Vec::new();
    // Issue #572: track destructuring patterns on plain `Param` ctor args so
    // the body sees the destructured names (TsParamProp can't be a destructure).
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &ctor.params {
        match param {
            ast::ParamOrTsParamProp::Param(p) => {
                let param_name = get_pat_name(&p.pat)?;
                let param_type = extract_param_type_with_ctx(&p.pat, Some(ctx));
                let param_default = get_param_default(ctx, &p.pat)?;
                let is_rest = is_rest_param(&p.pat);
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                ctx.shadow_native_instance_if_present(&param_name);
                ctx.shadow_native_module_if_present(&param_name);
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: param_default,
                    decorators: lower_decorators(ctx, &p.decorators),
                    is_rest,
                    arguments_object: None,
                });
                let inner_pat = if let ast::Pat::Assign(assign) = &p.pat {
                    assign.left.as_ref()
                } else {
                    &p.pat
                };
                if is_destructuring_pattern(inner_pat) {
                    destructuring_params.push((param_id, inner_pat.clone()));
                }
            }
            ast::ParamOrTsParamProp::TsParamProp(ts_prop) => {
                // Handle parameter properties (e.g., constructor(public x: number))
                // Capture the default like a plain param: `build_default_param_stmts`
                // (below) turns it into `if (p === undefined) p = <default>` so the
                // synthesized `this.field = p` assignment stores the default rather
                // than `undefined` when the arg is omitted. Without this, a defaulted
                // parameter property such as effect's MetricKeyImpl
                // `readonly tags: ReadonlyArray<...> = []` left `this.tags` null, and
                // `Hash.array(this.tags)` then threw "Cannot read properties of null
                // (reading 'length')". Issue #1757/#1758 (toward #321).
                let (param_name, param_type, param_default) = match &ts_prop.param {
                    ast::TsParamPropParam::Ident(ident) => {
                        let name = ident.id.sym.to_string();
                        let ty = ident
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                            .unwrap_or(Type::Any);
                        // `constructor(public x?: T)` — optional param props
                        // default to undefined, mirroring get_param_default.
                        let default = if ident.id.optional {
                            Some(Expr::Undefined)
                        } else {
                            None
                        };
                        (name, ty, default)
                    }
                    ast::TsParamPropParam::Assign(assign) => {
                        let name = get_pat_name(&assign.left)?;
                        let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                        // Lower the default before defining this param's local,
                        // matching the plain-`Param` ordering at get_param_default.
                        let default = Some(lower_expr(ctx, &assign.right)?);
                        (name, ty, default)
                    }
                };
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                ctx.shadow_native_instance_if_present(&param_name);
                ctx.shadow_native_module_if_present(&param_name);
                // Record this param for synthesizing `this.field = param` assignment
                param_prop_assignments.push((param_id, param_name.clone()));
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: param_default,
                    decorators: lower_decorators(ctx, &ts_prop.decorators),
                    is_rest: false, // TsParamProp cannot be a rest parameter
                    arguments_object: None,
                });
            }
        }
    }

    // #677: synthesize `arguments` if the body references it and no user
    // param already binds it (TsParamProp can't be a rest, so the only
    // conflicts come from explicit `arguments` params or other rest params).
    let user_has_arguments_param = params.iter().any(|p| p.name == "arguments");
    let needs_arguments_synth = !user_has_arguments_param
        && ctor
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params, true, false, true, Vec::new());
    }

    // Issue #572: generate destructuring extractions BEFORE lowering the
    // body so destructured names are in scope for identifier resolution.
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    if !destructuring_params.is_empty() {
        for (param_id, pat) in &destructuring_params {
            let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
            destructuring_stmts.extend(stmts);
        }
    }

    // Lower body — issue #569; constructor body may contain hoisted
    // inner function declarations that need PreallocateBoxes for sibling
    // captures.
    let mut body = if let Some(ref block) = ctor.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Synthesize `this.field = param` assignments for parameter properties.
    // In TypeScript, `constructor(public name: string)` automatically assigns
    // `this.name = name` at the start of the constructor body.
    if !param_prop_assignments.is_empty() {
        let assignments: Vec<Stmt> = param_prop_assignments
            .iter()
            .map(|(param_id, field_name)| {
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: field_name.clone(),
                    value: Box::new(Expr::LocalGet(*param_id)),
                })
            })
            .collect();
        // For a derived class, `this` is unusable until `super()` runs, so TS
        // emits the param-property assignments immediately AFTER the super()
        // call — not at the top. Prepending them (the historical behavior)
        // dropped the derived class's own param-props: e.g. SchemaAST's
        // `class OptionalType extends Type { constructor(type, readonly
        // isOptional, ...) { super(type, ...) } }` left `this.isOptional`
        // undefined, which cascaded into `typeAST(undefined)` reading `._tag`
        // during effect Schema init (#1758). Base classes (no super() call)
        // keep prepending. The default-param / destructuring prologues below
        // touch only params (not `this`), so they stay at the very top.
        if let Some(super_pos) = body
            .iter()
            .position(|s| matches!(s, Stmt::Expr(Expr::SuperCall(_) | Expr::SuperCallSpread(_))))
        {
            let tail = body.split_off(super_pos + 1);
            body.extend(assignments);
            body.extend(tail);
        } else {
            let mut synthetic_stmts = assignments;
            synthetic_stmts.append(&mut body);
            body = synthetic_stmts;
        }
    }

    // Issue #572: prepend destructuring extractions for ctor params (the
    // stmts were generated before body lowering so locals are in scope).
    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    // Prepend defaulted-parameter application: for every param with a
    // default, emit `if (param === undefined) { param = default; }` at the
    // very top of the constructor body. Needed for cross-module `new C(...)`
    // calls that pass fewer args than the constructor declares — the
    // codegen call site pads missing args with TAG_UNDEFINED, so without
    // body-side default application the param reads as `undefined`. The
    // in-module HIR `fill_default_arguments` pass already fills the args at
    // same-module call sites, so this check is a no-op there.
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;
    ctx.in_constructor_class = saved_ctor_class;

    Ok(Function {
        id: ctx.fresh_func(),
        name: format!("{}::constructor", class_name),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Issue #212: list outer-scope LocalIds referenced by a method/getter/
/// setter/constructor body. An id is "captured" when it's referenced inside
/// the body (or any nested closure inside the body — `collect_local_refs_*`
/// descends), but isn't one of the function's own params, isn't `this`, and
/// wasn't declared inside the body itself. Module-level ids are excluded
/// because codegen reads those directly from globals — they don't need
/// per-instance snapshotting.
///
/// `outer_scope_ids` is the snapshot of `ctx.locals` at the post-class
/// point (when this analysis runs). Refs that aren't in this set must
/// belong to inner closures' params/locals — `collect_local_refs_*`
/// descends into closure bodies indiscriminately, and without this filter
/// we'd wrongly capture inner closures' own arg ids.
pub fn collect_method_captures(
    func: &Function,
    outer_scope_ids: &std::collections::HashSet<LocalId>,
    module_level_ids: &std::collections::HashSet<LocalId>,
) -> Vec<LocalId> {
    let mut own_locals: std::collections::HashSet<LocalId> =
        func.params.iter().map(|p| p.id).collect();
    fn collect_let_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
        for s in stmts {
            match s {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_let_ids(then_branch, out);
                    if let Some(e) = else_branch {
                        collect_let_ids(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_ids(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                            out.insert(*id);
                        }
                    }
                    collect_let_ids(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_let_ids(body, out);
                    if let Some(c) = catch {
                        collect_let_ids(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_let_ids(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_let_ids(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    collect_let_ids(std::slice::from_ref(body.as_ref()), out)
                }
                _ => {}
            }
        }
    }
    collect_let_ids(&func.body, &mut own_locals);

    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for stmt in &func.body {
        crate::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    let mut captures: Vec<LocalId> = refs
        .into_iter()
        .filter(|id| {
            outer_scope_ids.contains(id)
                && !own_locals.contains(id)
                && !module_level_ids.contains(id)
        })
        .collect();
    captures.sort();
    captures.dedup();
    captures
}

/// Conservative outer-capture check used to gate `[Symbol.dispose]` /
/// `[Symbol.asyncDispose]` lowering: returns true when the method body
/// references any LocalId that isn't `this` or one of the method's own
/// parameters. Class-method-captures-outer-local has a pre-existing codegen
/// gap; for the dispose family we silently drop the method when this is true,
/// so test programs that previously compiled (with empty disposed output)
/// keep compiling.
#[allow(dead_code)]
fn method_body_captures_outer(func: &Function, ctx: &LoweringContext) -> bool {
    let mut own_locals: std::collections::HashSet<LocalId> =
        func.params.iter().map(|p| p.id).collect();
    // Also include `this` if it was registered (instance methods).
    if let Some(this_id) = ctx
        .locals
        .iter()
        .rev()
        .find(|(name, _, _)| name == "this")
        .map(|(_, id, _)| *id)
    {
        own_locals.insert(this_id);
    }
    // Locals defined inside the body (e.g., `let x = ...` inside the method)
    // also need to be treated as own-locals so they don't trip the capture
    // check. Walk the body collecting Let ids.
    fn collect_let_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
        for s in stmts {
            match s {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_let_ids(then_branch, out);
                    if let Some(e) = else_branch {
                        collect_let_ids(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_ids(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                            out.insert(*id);
                        }
                    }
                    collect_let_ids(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_let_ids(body, out);
                    if let Some(c) = catch {
                        collect_let_ids(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_let_ids(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_let_ids(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    collect_let_ids(std::slice::from_ref(body.as_ref()), out)
                }
                _ => {}
            }
        }
    }
    collect_let_ids(&func.body, &mut own_locals);

    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for stmt in &func.body {
        crate::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    refs.iter().any(|id| !own_locals.contains(id))
}

pub fn lower_class_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        // Numeric-literal method name (`42() {}`): the registration key (and
        // func name) is the canonical ToString of the value.
        ast::PropName::Num(n) => crate::lower::number_to_js_key(n.value),
        ast::PropName::Computed(computed) if is_symbol_iterator_key(&computed.expr) => {
            "@@iterator".to_string()
        }
        ast::PropName::Computed(computed)
            if is_inspect_custom_key(ctx, &computed.expr)
                && !method.is_static
                && matches!(method.kind, ast::MethodKind::Method) =>
        {
            // Refs #1248: `[util.inspect.custom]() {}` on a class — rename to
            // a stable string key so the runtime's vtable picks it up.
            "__perry_inspect_custom__".to_string()
        }
        ast::PropName::Computed(computed) => {
            // Well-known symbols (hasInstance, toStringTag, toPrimitive,
            // asyncIterator) get a synthetic `@@<short>` name. The caller
            // is responsible for renaming / lifting the returned Function
            // as needed — see the well-known handling in lower_class_decl.
            // `dispose` / `asyncDispose` get stable string names so the
            // using-block desugarer can dispatch via plain method-call.
            if let Some(wk) = symbol_well_known_key(&computed.expr) {
                match wk {
                    "dispose" => "__perry_dispose__".to_string(),
                    "asyncDispose" => "__perry_async_dispose__".to_string(),
                    other => format!("@@{}", other),
                }
            } else {
                return Err(anyhow!("Unsupported method key"));
            }
        }
        _ => return Err(anyhow!("Unsupported method key")),
    };
    lower_class_method_with_name(ctx, method, name)
}

pub fn lower_class_method_with_name(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    name: String,
) -> Result<Function> {
    // Lower decorators from the method's function
    let decorators = lower_decorators(ctx, &method.function.decorators);

    // Extract method-level type parameters (e.g., method<U>(x: U): T)
    // Note: Class-level type params are already in scope from lower_class_decl
    let type_params = method
        .function
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter method's type param scope (nested inside class scope if applicable)
    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    ctx.enter_strict_mode(true);

    // Add 'this' for instance methods
    if !method.is_static {
        ctx.define_local("this".to_string(), Type::Any);
    }

    // Lower parameters with type extraction (using context for type param resolution)
    let mut params = Vec::new();
    // Issue #572: track destructuring patterns so the body sees real bindings
    // rather than reading from a synthetic `__obj_destruct_*` local that the
    // method body can't reach by name.
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        // TypeScript's `this: T` is a TYPE-only marker (SWC emits it as a
        // regular `Param { pat: Ident("this") }`), so skip it — it must not
        // become a runtime parameter. `this` is already bound above via
        // `define_local("this", ...)` for instance methods. Mirrors the
        // `fn_decl.rs` / `expr_object.rs` sites. Without this skip, a class
        // method `m(this: C, fin) {}` is lowered as a 2-arg function and the
        // real `fin` arg lands in the `this` slot (#321).
        if param_name == "this" {
            continue;
        }
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            decorators: lower_decorators(ctx, &param.decorators),
            is_rest,
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
        // Mirror the lower_fn_decl shape: an `Assign` pattern can wrap a
        // destructure (e.g. `({ a } = {}) => ...`). Unwrap before testing.
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }
    // #677: synthesize `arguments` if the method body — or any parameter
    // DEFAULT expression (`method(x = arguments[2]) {}`) — references it.
    // Appended BEFORE the defaults are lowered below so `arguments` inside a
    // default resolves to the synthetic local instead of an unknown global.
    let user_has_arguments_param = method
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let needs_arguments_synth = !user_has_arguments_param
        && (method
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false)
            || params_use_arguments(&method.function.params));
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params, true, false, true, Vec::new());
    }

    for (param, pat) in params.iter_mut().zip(default_param_pats.iter()) {
        param.default = get_param_default(ctx, pat)?;
    }

    // Extract return type (with context). Phase 4: when the method has no
    // explicit annotation, fall back to body-based inference after body
    // lowering so parameters and locals are visible to `infer_type_from_expr`.
    let has_explicit_return_annotation = method.function.return_type.is_some();
    let mut return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Pre-register the explicit-annotated return type BEFORE lowering the
    // body so a sibling method appearing later in the class (or this method
    // calling itself recursively) can resolve `this.method()` through
    // `infer_call_return_type`. The post-loop registration in
    // `lower_class_decl` only sees lowered methods, which is too late for
    // intra-class call-site inference inside the body we're about to lower.
    if has_explicit_return_annotation && !matches!(return_type, Type::Any) {
        if let Some(class_name) = ctx.current_class.clone() {
            ctx.register_class_method_return_type(class_name, name.clone(), return_type.clone());
        }
    }

    // Issue #572: generate destructuring extractions BEFORE lowering the
    // body, so the destructured names land in `ctx.locals` and identifier
    // references inside the body resolve to those LocalIds (matching the
    // order used by lower_fn_decl). Doing this after body lowering would
    // produce "unknown identifier" warnings + GlobalGet(0) refs.
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    if !destructuring_params.is_empty() {
        for (param_id, pat) in &destructuring_params {
            let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
            destructuring_stmts.extend(stmts);
        }
    }

    // Lower body — see issue #569.
    let mut body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Capture the destructuring-prologue length before it is drained into the
    // body, so generator methods can replay param binding synchronously at call
    // time (see the `gen_param_prologue_len` recording below).
    let destructuring_prologue_len = destructuring_stmts.len();
    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    // Issue #235: prepend `if (param === undefined) param = default;` for
    // every default-value param so a caller passing fewer args (which
    // codegen now pads with TAG_UNDEFINED — see `lower_call.rs` dispatch
    // tower) gets the declared default instead of literal `undefined`.
    // Pre-fix the desugaring fired only on free functions (`lower_fn_decl`)
    // and constructors, never on instance/static class methods. The
    // standalone Calc.add(a, b = 10) regression case below printed `b` as
    // `undefined` post-padding because the method body just did `return a + b`
    // with no default check.
    let default_stmts = build_default_param_stmts(&params);
    let default_prologue_len = default_stmts.len();
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.extend(body);
        body = new_body;
    }

    // Phase 4 (expansion): body-based return-type inference for unannotated
    // methods. Same pattern as `lower_fn_decl`: skip when annotation is
    // present or when the method is a generator; wrap inferred type in
    // Promise<T> for async methods. Feeds the class's `Function.return_type`
    // which is then consumed by call-site inference at receiver.method()
    // sites (currently limited — bare-method call-site inference isn't
    // wired through `infer_call_return_type` yet; this commit only
    // populates the field so class methods stop showing Type::Any when
    // callers inspect them via receiver_class_name + class.methods lookup).
    if !has_explicit_return_annotation
        && matches!(return_type, Type::Any)
        && !method.function.is_generator
    {
        if let Some(ref block) = method.function.body {
            if let Some(inferred) = infer_body_return_type(&block.stmts, ctx) {
                return_type = if method.function.is_async {
                    Type::Promise(Box::new(inferred))
                } else {
                    inferred
                };
            }
        }
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    // Exit method's type param scope
    ctx.exit_type_param_scope();

    let func_id = ctx.fresh_func();
    // Record the param-prologue length for generator methods so the generator
    // transform runs param binding (default guards + destructuring) synchronously
    // at call time per spec FunctionDeclarationInstantiation order. Without this,
    // a `*method`/`async *method` with a destructuring/default param leaves the
    // binding inside the lazy state-machine body, so a throwing default
    // initializer never fires at call time (test262 class/dstr async-gen-meth-*).
    if method.function.is_generator {
        let prologue_len = default_prologue_len + destructuring_prologue_len;
        if prologue_len > 0 {
            ctx.gen_param_prologue_len.insert(func_id, prologue_len);
        }
    }

    Ok(Function {
        id: func_id,
        name,
        type_params,
        params,
        return_type,
        body,
        is_async: method.function.is_async,
        is_generator: method.function.is_generator,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators,
        was_plain_async: false,
        was_unrolled: false,
    })
}

/// Lower a getter method (get propertyName(): Type { ... })
pub fn lower_getter_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("get_{}", ident.sym),
        ast::PropName::Str(s) => format!("get_{}", s.value.as_str().unwrap_or("")),
        // Numeric-literal getter (`get 0()`): synthetic internal symbol; the
        // accessor is registered under the canonical numeric prop key by the
        // caller (lower_class_decl), not this name.
        ast::PropName::Num(n) => format!("get_{}", n.value),
        ast::PropName::Computed(computed) => {
            // Well-known symbol getters (e.g., `get [Symbol.toStringTag]()`)
            // get a synthetic `get_@@<short>` name. The caller is
            // responsible for lifting / renaming as needed.
            if let Some(wk) = symbol_well_known_key(&computed.expr) {
                format!("get_@@{}", wk)
            } else {
                return Err(anyhow!("Unsupported getter key"));
            }
        }
        _ => return Err(anyhow!("Unsupported getter key")),
    };
    lower_getter_method_with_name(ctx, method, name)
}

pub fn lower_getter_method_with_name(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    name: String,
) -> Result<Function> {
    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    ctx.enter_strict_mode(true);

    // Add 'this' for instance getters
    ctx.define_local("this".to_string(), Type::Any);

    // Getters have no parameters

    // Extract return type. Phase 4: body-based inference when no annotation.
    let has_explicit_return_annotation = method.function.return_type.is_some();
    let mut return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower body — see issue #569.
    let body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Phase 4: getters can't be async/generator by JS syntax, so just the
    // plain body-walk + unify path. Feeds `class.getters[i].1.return_type`
    // which `receiver_class_name`-style codegen consults to pick Return
    // types through `obj.prop` chains.
    if !has_explicit_return_annotation && matches!(return_type, Type::Any) {
        if let Some(ref block) = method.function.body {
            if let Some(inferred) = infer_body_return_type(&block.stmts, ctx) {
                return_type = inferred;
            }
        }
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params: Vec::new(),
        return_type,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a setter method (set propertyName(value: Type) { ... })
pub fn lower_setter_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("set_{}", ident.sym),
        ast::PropName::Str(s) => format!("set_{}", s.value.as_str().unwrap_or("")),
        // Numeric-literal setter (`set 0(v)`): synthetic internal symbol;
        // registered under the canonical numeric prop key by the caller.
        ast::PropName::Num(n) => format!("set_{}", n.value),
        _ => return Err(anyhow!("Unsupported setter key")),
    };
    lower_setter_method_with_name(ctx, method, name)
}

pub fn lower_setter_method_with_name(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    name: String,
) -> Result<Function> {
    let scope_mark = ctx.enter_scope();
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    ctx.enter_strict_mode(true);

    // Add 'this' for instance setters
    ctx.define_local("this".to_string(), Type::Any);

    // Setters have exactly one parameter
    let mut params = Vec::new();
    // Issue #572: setter param can be a destructuring pattern (`set v({ x }) {...}`).
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        // Skip the TypeScript `this: T` TYPE-only marker (see the method loop
        // above). `this` is already bound for instance setters.
        if param_name == "this" {
            continue;
        }
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        // #5703: a setter with a defaulted parameter (`set a(_ = expr)`) must
        // apply the default when invoked with `undefined` (e.g. `C.a =
        // undefined`). Previously the setter param dropped its default
        // (`default: None`) and emitted no prologue, so the default expression
        // never ran. Mirror the constructor/method path: lower the default
        // BEFORE defining the param local so its references resolve to the
        // enclosing scope rather than the param or the body's hoisted `var`s —
        // i.e. the separate parameter/body VariableEnvironment the spec
        // mandates (test262 scope-*-setter-paramsbody-var-open) — then emit the
        // `if (param === undefined) param = <default>` prologue below.
        let param_default = get_param_default(ctx, &param.pat)?;
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        });
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    // Generate destructuring stmts BEFORE body lowering (issue #572).
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    // Lower body — see issue #569.
    let mut body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    // #5703: prepend `if (param === undefined) param = <default>` for a
    // defaulted setter parameter, so an under-supplied / `undefined` assignment
    // (`C.a = undefined`) applies the default. Runs before any destructuring
    // prologue (which destructures the already-defaulted param).
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub fn lower_class_prop(ctx: &mut LoweringContext, prop: &ast::ClassProp) -> Result<ClassField> {
    // Computed property keys (`[Symbol.for("k")]`, `[Parent.Symbol.X]`, etc.)
    // can't be reduced to a string at compile time — the key expression is
    // evaluated at construction time. We capture the lowered key expression
    // in `key_expr` and synthesize a placeholder `name` for HIR identity
    // (string-keyed lookup paths skip these fields via `key_expr.is_some()`).
    let (name, key_expr) = match &prop.key {
        ast::PropName::Ident(ident) => (ident.sym.to_string(), None),
        ast::PropName::Str(s) => (s.value.as_str().unwrap_or("").to_string(), None),
        // Numeric field keys (`0 = 'bar'`, `1e-7;`) use the canonical JS number
        // -to-string conversion so reads via `c[0]` / `c['1e-7']` agree.
        ast::PropName::Num(n) => (crate::lower::number_to_js_key(n.value), None),
        ast::PropName::BigInt(b) => (b.value.to_string(), None),
        ast::PropName::Computed(c) => {
            let key = lower_expr(ctx, &c.expr)?;
            // Synthetic name — uniqueness within a class is enforced by the
            // caller appending to fields/static_fields in source order; the
            // HIR's field-list iterators that key on `name` will still see
            // distinct entries because each computed-key field lowers in its
            // own call.
            let synth = format!("__computed_field_{}_{}", c.span.lo.0, c.span.hi.0);
            (synth, Some(key))
        }
    };

    // Extract type from type annotation (using context for class type param resolution).
    // Issue #305: when the annotation is absent, fall back to inferring the type from the
    // initializer (`= new Map<K,V>()`, `= new Set<T>()`, `= []`, ...) so the late
    // `register_class_field_types` call doesn't clobber the early-registered correct type
    // with Type::Any.
    let ty = match prop.type_ann.as_ref() {
        Some(ann) => extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)),
        None => prop
            .value
            .as_ref()
            .map(|v| infer_type_from_expr(v, ctx))
            .unwrap_or(Type::Any),
    };

    // Lower initializer expression if present. Mark the field-initializer
    // context so a direct `eval` in the initializer rejects `arguments`
    // (PerformEval early error — field initializers have no arguments object).
    // Also set `in_nonarrow_fn` so a direct eval in the initializer allows
    // `new.target` (ES2025 §sec-performeval-rules-in-initializer: the eval
    // runs "inside a function" for new.target purposes).
    // NamedEvaluation: an anonymous function/arrow/class initializer takes
    // the field's name (`static fromArgs = function(){}` → `.name ===
    // "fromArgs"`, test262 elements/static-field-anonymous-function-name).
    // Computed keys (key_expr) have no compile-time name to confer.
    let saved_field_init = ctx.in_class_field_init;
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_class_field_init = true;
    ctx.in_nonarrow_fn = true;
    let init = prop
        .value
        .as_ref()
        .map(|e| {
            if key_expr.is_none() && crate::lower::expr_assign::rhs_accepts_assignment_name(e) {
                let old = ctx.assignment_inferred_name.replace(name.clone());
                let result = lower_expr(ctx, e);
                ctx.assignment_inferred_name = old;
                result
            } else {
                lower_expr(ctx, e)
            }
        })
        .transpose();
    ctx.in_class_field_init = saved_field_init;
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;
    let init = init?;

    Ok(ClassField {
        name,
        key_expr,
        ty,
        init,
        is_private: false, // TODO: check accessibility
        is_readonly: prop.readonly,
        decorators: lower_decorators(ctx, &prop.decorators),
    })
}
