//! `lower_class_from_ast` — lowering a class EXPRESSION (as opposed to a
//! class declaration statement, handled in `class_decl.rs` proper) to HIR.
//! Split out of `class_decl.rs` to keep it under the 2000-line file gate.
//! Behaviour is unchanged; `use super::*` reaches the shared imports.

use super::*;

/// Lower a class expression (ast::Class) to HIR.
/// Used for anonymous class expressions like `new (class extends Command { ... })()`.
pub(crate) fn lower_class_from_ast(
    ctx: &mut LoweringContext,
    class: &ast::Class,
    name: &str,
    is_exported: bool,
) -> Result<Class> {
    validate_legacy_decorator_surface(class, name)?;
    validate_class_element_early_errors(class, name)?;
    let class_id = match ctx.lookup_class(name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.to_string(), id);
            id
        }
    };
    capture_class_source(ctx, class_id, class);

    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.to_string());
    let old_class_scope_depth = ctx.current_class_scope_depth.replace(ctx.scope_depth);
    let old_inner_name = ctx.current_class_inner_name.take();
    // A class-expression caller stashes the source ident here; fall back
    // to the (possibly synthetic) registration name when absent.
    let explicit_inner_name = ctx.pending_class_inner_name.take();
    ctx.current_class_inner_name = explicit_inner_name
        .clone()
        .or_else(|| Some(name.to_string()));
    let old_is_derived = ctx.current_class_is_derived;
    ctx.current_class_is_derived = class.super_class.is_some();

    // Private-name scope for this class-expression body (see lower_class_decl).
    ctx.push_private_scope(super::build_private_scope(class, name, class_id));

    // Issue #562: same as the parallel `lower_class_decl` arm — track the
    // parent class identifier so super({...}) controller-param pre-scan
    // fires for stream subclasses.
    let old_super_ident = ctx.current_class_super_ident.take();
    ctx.current_class_super_ident = match class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => Some(ident.sym.to_string()),
        _ => None,
    };

    let type_params = class
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    ctx.enter_type_param_scope(&type_params);

    // #5437: parent Ident shadowed by an in-scope lexical local? (See the
    // matching computation in `lower_class_decl`.) Lets codegen prefer the
    // dynamic local over a NAME-keyed built-in special case.
    let heritage_lexically_shadowed = match class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => {
            let n = ident.sym.to_string();
            !ctx.class_renames.contains_key(&n) && ctx.locals.lookup(&n).is_some()
        }
        _ => false,
    };

    let (extends, extends_name, native_extends, extends_expr) = if let Some(ref super_class) =
        class.super_class
    {
        if explicit_inner_name
            .as_deref()
            .is_some_and(|inner| is_class_self_heritage(super_class, inner))
        {
            (
                None,
                None,
                None,
                Some(Box::new(crate::lower::throw_reference_error_expr(
                    "js_throw_reference_error_this_before_super",
                ))),
            )
        } else if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            let canonical_parent_name = canonical_native_parent_name(ctx, &parent_name)
                .unwrap_or(&parent_name)
                .to_string();
            let native_parent = match canonical_parent_name.as_str() {
                "EventEmitter" => Some(("events".to_string(), "EventEmitter".to_string())),
                "EventEmitterAsyncResource" => Some((
                    "events".to_string(),
                    "EventEmitterAsyncResource".to_string(),
                )),
                "AsyncLocalStorage" => {
                    Some(("async_hooks".to_string(), "AsyncLocalStorage".to_string()))
                }
                "AsyncResource" => Some(("async_hooks".to_string(), "AsyncResource".to_string())),
                "WebSocketServer" => Some(("ws".to_string(), "WebSocketServer".to_string())),
                // Issue #562: keep in lockstep with the parallel arm in
                // `lower_class_decl` above.
                "ReadableStream" => {
                    Some(("readable_stream".to_string(), "ReadableStream".to_string()))
                }
                "WritableStream" => {
                    Some(("writable_stream".to_string(), "WritableStream".to_string()))
                }
                "TransformStream" => Some((
                    "transform_stream".to_string(),
                    "TransformStream".to_string(),
                )),
                // #1545: classic node:stream base classes — keep in lockstep
                // with the parallel arm in `lower_class_decl` above. Gated on
                // `is_genuine_node_stream_parent` so a userland stream-shim
                // binding (readable-stream's `Transform`) falls through to the
                // dynamic `extends_expr` parent path.
                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                    if is_genuine_node_stream_parent(ctx, &parent_name) =>
                {
                    Some(("node_stream".to_string(), canonical_parent_name.clone()))
                }
                _ => None,
            };
            // A lexical local binding shadowing the parent name must win over the
            // native/static parent — the in-scope local IS the real parent value.
            // Check it BEFORE `native_parent` so e.g. `const EventEmitter = …;
            // const C = class extends EventEmitter {}` routes through the dynamic
            // `extends_expr` path (the local) instead of recording the native
            // `events` parent. ESM imports are NOT in `ctx.locals`, so genuine
            // `extends EventEmitter` (imported) still takes the native path.
            //
            // #10623: same CJS-wrapper carve-out as the class-declaration arm
            // above — see its comment for the full rationale.
            let require_native_reexport = ctx
                .require_destructured_native_locals
                .get(&parent_name)
                .is_some_and(|key| *key == canonical_parent_name);
            let locally_shadowed = !ctx.class_renames.contains_key(&parent_name)
                && ctx.locals.lookup(&parent_name).is_some()
                && !require_native_reexport;
            if native_parent.is_some() && !locally_shadowed {
                (None, Some(canonical_parent_name), native_parent, None)
            } else if locally_shadowed {
                // #5437 (Next.js p-queue `PQueue` inside a minified bundle): a
                // class EXPRESSION whose parent Ident is an IN-SCOPE LOCAL
                // (`const t = require("events"); … class extends t {…}`) must
                // bind to that LEXICAL local — not to an unrelated module-global
                // class that happens to share the (minified, single-letter)
                // name. The static `lookup_class(parent_name)` path keys
                // codegen's `super()` on a module-wide `HashMap<name, &Class>`;
                // in a turbopack chunk dozens of distinct webpack-factory
                // classes are all named `t`/`u`/`i`, so that map keeps ONE `t`
                // (whichever registered last) and `super()` inlines the WRONG
                // class's constructor. The bundle's p-queue `PQueue extends t`
                // (eventemitter3) resolved `t` to superstruct's `StructError`
                // base, so `new PQueue()` ran StructError's destructuring ctor
                // on the (undefined) options arg → "Cannot convert undefined or
                // null to object" → HTTP 500 on the dynamic page routes.
                //
                // When the parent name is bound by a local in THIS body's scope,
                // route through the dynamic `extends_expr` path: lower the Ident
                // as a runtime value (the lexically-correct local), register the
                // parent edge dynamically, and let `super()` invoke the real
                // parent value via `js_fetch_or_value_super` (which already
                // tolerates native / closure / class-ref / builtin parents).
                // Gated on `!class_renames.contains_key` so the #5437
                // sibling-rename path above still wins when a scope-local class
                // rename exists (that disambiguation is exact). Pure-Ident
                // module-global heritage (no shadowing local) is unaffected —
                // `ctx.locals.lookup` returns `None` for a class name.
                // Do NOT set a static `extends` (parent_cid) OR `extends_name`
                // here: the only candidate is `lookup_class(parent_name)`, the
                // wrong same-named module-global class we deliberately avoid — and
                // a retained `extends_name` is re-resolved back to it by the
                // static parent-chain walks (layout / parent-edge / inherited-
                // method / vtable / type-facts), corrupting the subclass. The
                // dynamic `extends_expr` path registers the correct parent edge at
                // runtime via `RegisterClassParentDynamic` + `function_class_id`.
                match lower_class_heritage_expr(ctx, super_class) {
                    Ok(expr) => (None, None, None, Some(Box::new(expr))),
                    Err(_) => (None, None, None, None),
                }
            } else {
                // #5437: resolve the parent through active scope-local class
                // renames so a class EXPRESSION extending a disambiguated
                // same-named sibling (`f` -> `f$0`) binds to the right class.
                // See the matching fix in `lower_class_decl` above.
                let parent_name = ctx.resolve_class_name(&parent_name);
                let parent_cid = ctx.lookup_class(&parent_name);
                if parent_cid.is_none() {
                    // Issue #711 part 2: see the parallel arm in
                    // `lower_class_decl` above. Unknown Ident super-class
                    // falls through to extends_expr capture so a
                    // function-with-prototype value can be resolved at
                    // runtime via `function_class_id`.
                    match lower_class_heritage_expr(ctx, super_class) {
                        Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (None, Some(parent_name), None, None),
                    }
                } else if ctx.scope_depth > 0 && ctx.locals.lookup(ident.sym.as_ref()).is_some() {
                    // A function-local class declaration is a fresh class
                    // object each time its enclosing function runs. Preserve
                    // its static id for method/layout analysis, but also
                    // record the evaluated local as the actual superclass.
                    // Otherwise a fresh child class expression links to the
                    // shared template prototype and loses writes such as
                    // `Base.prototype.name = tag` (Effect TaggedError).
                    match lower_class_heritage_expr(ctx, super_class) {
                        Ok(expr) => (parent_cid, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (parent_cid, Some(parent_name), None, None),
                    }
                } else {
                    (parent_cid, Some(parent_name), None, None)
                }
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            // Refs #488 drizzle-sqlite: try cross-module class lookup. See
            // the matching arm in `lower_class_decl` (above) for the full
            // rationale — without this, the parent link is lost and
            // inherited methods don't reach instances.
            let parent_name = extract_member_class_name(member);
            // Issue #4908: avoid a self-referential parent edge when the
            // member's trailing property equals the subclass's own name
            // (`class Agent extends http.Agent`). See the matching guard in
            // `lower_class_decl` above — a self-link loops codegen's
            // parent-chain walk forever. Leave the class parentless, matching
            // the non-colliding native-member-base behavior.
            if parent_name == name {
                (None, None, None, None)
            } else if parent_name == "default" {
                // `class X extends _mod.default` — the interop ESM
                // default-export-class pattern. Keep in lockstep with the
                // matching `.default` arm in `lower_class_decl` above: route
                // through `extends_expr` so `super()` re-evaluates the alias
                // at construction time and the parent edge is registered.
                match lower_class_heritage_expr(ctx, super_class) {
                    Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (None, Some(parent_name), None, None),
                }
            } else {
                // Named cross-module member-extends — route through `extends_expr`
                // UNCONDITIONALLY so `super()` runs the parent ctor at runtime even
                // when the parent isn't in codegen's class table / not yet lowered.
                // Keep in lockstep with the matching arm in `lower_class_decl`
                // (wall 48: NodeNextRequest extends _index.BaseNextRequest).
                let resolved = ctx.lookup_class(&parent_name);
                match lower_class_heritage_expr(ctx, super_class) {
                    Ok(expr) => (resolved, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (resolved, Some(parent_name), None, None),
                }
            }
        } else {
            // Issue #711: see the matching arm in `lower_class_decl` above
            // for the full rationale. Capture the lowered extends
            // expression so codegen can evaluate it at the class
            // declaration site and call
            // `js_register_class_parent_dynamic` at runtime.
            match lower_class_heritage_expr(ctx, super_class) {
                Ok(expr) => (None, None, None, Some(Box::new(expr))),
                Err(_) => (None, None, None, None),
            }
        }
    } else {
        (None, None, None, None)
    };

    // Issue #10486: mirrors the capture-forwarding fallback in
    // `lower_class_decl` above (see its comment for the full rationale) —
    // a class EXPRESSION extending a lexically-local capture-bearing class
    // EXPRESSION (`const Base = class {…}; const Sub = class extends Base
    // {…}`) needs the alias-resolved heritage identifier for capture
    // lookup even when `extends_name` was deliberately left None for
    // class-registry resolution.
    // See the matching guard in `lower_class_decl` above: skip the
    // fallback when this class expression has its own explicit
    // constructor (its `super(...)` already forwards correctly).
    let has_own_constructor = class
        .body
        .iter()
        .any(|m| matches!(m, ast::ClassMember::Constructor(_)));
    let capture_parent_name: Option<String> = extends_name.clone().or_else(|| {
        if has_own_constructor {
            return None;
        }
        class.super_class.as_deref().and_then(|sc| match sc {
            ast::Expr::Ident(ident) => {
                let raw = ident.sym.to_string();
                Some(ctx.resolve_class_alias(&raw).unwrap_or(raw))
            }
            _ => None,
        })
    });

    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class.body {
        match member {
            // See note above: static getters/setters are not callable methods.
            ast::ClassMember::Method(method)
                if method.is_static && matches!(method.kind, ast::MethodKind::Method) =>
            {
                if let ast::PropName::Ident(ident) = &method.key {
                    static_method_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateMethod(method)
                if method.is_static && matches!(method.kind, ast::MethodKind::Method) =>
            {
                static_method_names.push(format!("#{}", method.key.name));
            }
            ast::ClassMember::ClassProp(prop) if prop.is_static && !prop.declare => {
                if let ast::PropName::Ident(ident) = &prop.key {
                    static_field_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                static_field_names.push(format!("#{}", prop.key.name));
            }
            _ => {}
        }
    }
    ctx.register_class_statics(name.to_string(), static_field_names, static_method_names);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();
    // Parallel staticness, so `record_class_accessor` can tell a static
    // accessor from an instance one with the same name.
    let mut getter_statics: Vec<bool> = Vec::new();
    let mut setter_statics: Vec<bool> = Vec::new();
    let mut static_accessor_names: Vec<String> = Vec::new();
    let mut static_accessor_fn_ids: Vec<FuncId> = Vec::new();
    let mut computed_members = Vec::new();
    let mut seen_generic_computed_member = false;

    for (member_index, member) in class.body.iter().enumerate() {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                if let Some(computed) = generic_computed_member_key(ctx, method) {
                    computed_members.push(lower_generic_computed_class_member(
                        ctx,
                        method,
                        computed,
                        member_index,
                    )?);
                    seen_generic_computed_member = true;
                    continue;
                }
                let (prop_name, can_source_order_register) = match &method.key {
                    ast::PropName::Ident(ident) => (ident.sym.to_string(), true),
                    ast::PropName::Str(s) => (s.value.as_str().unwrap_or("").to_string(), true),
                    // Numeric-literal member names — see the parallel arm in
                    // `lower_class_decl`. Canonical ToString of the value.
                    ast::PropName::Num(n) => (crate::lower::number_to_js_key(n.value), true),
                    // `[Symbol.iterator]() {}` / `*[Symbol.iterator]() {}` on a
                    // class *expression* — mirror the declaration path so
                    // `new (class { *[Symbol.iterator]() {…} })()` is iterable
                    // for spread, `Array.from`, destructuring, and manual
                    // `obj[Symbol.iterator]()` calls (#5128). The generator lift
                    // happens in the `Method` arm below.
                    ast::PropName::Computed(computed) if is_symbol_iterator_key(&computed.expr) => {
                        ("@@iterator".to_string(), false)
                    }
                    ast::PropName::Computed(computed)
                        if is_inspect_custom_key(ctx, &computed.expr)
                            && !method.is_static
                            && matches!(method.kind, ast::MethodKind::Method) =>
                    {
                        // Refs #1248: see class_decl.rs Method handling above.
                        ("__perry_inspect_custom__".to_string(), false)
                    }
                    // Other well-known-symbol keys (`[Symbol.asyncIterator]`,
                    // `[Symbol.toPrimitive]`, `[Symbol.dispose]` /
                    // `[Symbol.asyncDispose]`, `static [Symbol.hasInstance]`,
                    // `get [Symbol.toStringTag]`) on a class *expression* —
                    // same handling as the declaration path, via the shared
                    // helper. Pre-fix these fell through `_ => continue` and
                    // were silently dropped, so e.g. `for await (… of new (C =
                    // class { [Symbol.asyncIterator]() {…} })())` threw
                    // `TypeError: value is not iterable`.
                    ast::PropName::Computed(_) => {
                        match lower_well_known_computed_method(ctx, method, name)? {
                            Some(WellKnownComputedMethod::Rename(renamed)) => (renamed, false),
                            Some(
                                WellKnownComputedMethod::Lifted
                                | WellKnownComputedMethod::Unsupported,
                            )
                            | None => continue,
                        }
                    }
                    _ => continue,
                };
                match method.kind {
                    ast::MethodKind::Getter => {
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_getter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx,
                                method,
                                &prop_name,
                                member_index,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        record_class_accessor(
                            &mut getters,
                            &mut getter_statics,
                            prop_name,
                            func,
                            method.is_static,
                        );
                    }
                    ast::MethodKind::Setter => {
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_setter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx,
                                method,
                                &prop_name,
                                member_index,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        record_class_accessor(
                            &mut setters,
                            &mut setter_statics,
                            prop_name,
                            func,
                            method.is_static,
                        );
                    }
                    ast::MethodKind::Method => {
                        let mut func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_class_method(ctx, method)
                        })?;
                        // `*[Symbol.iterator]()` — lift to a top-level generator
                        // and register a synthetic `@@iterator` wrapper (#5128),
                        // exactly as the class-declaration path does above.
                        if prop_name == "@@iterator" && func.is_generator && !method.is_static {
                            let wrapper = synthesize_symbol_iterator_wrapper(ctx, name, &mut func);
                            let ast::PropName::Computed(computed) = &method.key else {
                                unreachable!("@@iterator generator key must be computed");
                            };
                            // The computed-symbol registration installs the
                            // runtime dispatch alias too. Registering the wrapper
                            // as a string method also exposed an own "@@iterator"
                            // property that the source never declared (#9788).
                            computed_members.push(ClassComputedMember {
                                key_expr: lower_expr(ctx, &computed.expr)?,
                                function: wrapper,
                                is_static: false,
                                kind: ClassComputedMemberKind::Method,
                                source_order: member_index,
                            });
                            continue;
                        }
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx,
                                method,
                                &prop_name,
                                member_index,
                            )?);
                        }
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                }
            }
            ast::ClassMember::ClassProp(prop) => {
                // `declare` and `abstract` fields are type-only: TypeScript
                // erases them entirely (`node --experimental-strip-types`
                // emits no runtime slot). Materializing an abstract base-class
                // field creates a phantom slot that shadows the concrete
                // subclass initializer of the same name — a base/union-typed
                // read then resolves to the (undefined) base slot. Skip both.
                if prop.declare || prop.is_abstract {
                    continue;
                }
                // Computed-key fields (`[Symbol.for("k")] = init`) flow through
                // here for both instance AND static positions.
                // `lower_class_prop` captures the key expression in
                // `ClassField.key_expr` for runtime evaluation. Refs #420 —
                // drizzle's `static [entityKind] = "Table"` is the canonical
                // static-computed-key pattern; codegen's `init_static_fields`
                // detects `key_expr.is_some()` and emits a runtime
                // registration into the class-static-symbol side table.
                let field = lower_class_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                let field = lower_private_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateMethod(method) => {
                if method.function.body.is_none() {
                    continue;
                }
                match method.kind {
                    ast::MethodKind::Method => {
                        let func = lower_private_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                    ast::MethodKind::Getter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_getter(ctx, method)?;
                        // Static private accessor — register on the static
                        // side (see the matching arm in `lower_class_decl`).
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        record_class_accessor(
                            &mut getters,
                            &mut getter_statics,
                            prop_name,
                            func,
                            method.is_static,
                        );
                    }
                    ast::MethodKind::Setter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_setter(ctx, method)?;
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        record_class_accessor(
                            &mut setters,
                            &mut setter_statics,
                            prop_name,
                            func,
                            method.is_static,
                        );
                    }
                }
            }
            ast::ClassMember::StaticBlock(block) => {
                let scope_mark = ctx.enter_scope();
                let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
                ctx.in_nonarrow_fn = true;
                // A static block is its own var-scope (OrdinaryFunctionCreate
                // per ClassStaticBlockDefinitionEvaluation): `lower_block_stmt`
                // only lowers nested statements without hoisting `var`s to this
                // boundary, so a `var` declared in one block leaked into the
                // next block/module scope instead of staying local (test262
                // static-init-scope-var-close.js).
                let body = lower_fn_body_block_stmt(ctx, &block.body)?;
                ctx.exit_scope(scope_mark);
                ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

                let block_idx = static_methods
                    .iter()
                    .filter(|m| m.name.starts_with("__perry_static_init_"))
                    .count();
                let synthetic_name = format!("__perry_static_init_{}", block_idx);
                static_methods.push(Function {
                    id: ctx.fresh_func(),
                    name: synthetic_name,
                    type_params: Vec::new(),
                    params: Vec::new(),
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
                });
            }
            _ => {}
        }
    }

    // `this` in static field initializers — see the matching substitution in
    // `lower_class_decl` above.
    for sf in &mut static_fields {
        if let Some(init) = &mut sf.init {
            crate::analysis::substitute_lexical_this_in_expr(
                init,
                &Expr::ClassRef(name.to_string()),
            );
        }
    }

    ctx.exit_type_param_scope();
    // Issue #562: see the parallel site in `lower_class_decl` — register
    // native_extends so subclass instances of the three Web Stream base
    // classes route through the parent stream module's dispatch table.
    if let Some((module, class)) = native_extends.as_ref() {
        ctx.register_class_native_extends(name.to_string(), module.clone(), class.clone());
    }
    ctx.current_class = old_class;
    ctx.current_class_scope_depth = old_class_scope_depth;
    ctx.current_class_inner_name = old_inner_name;
    ctx.current_class_is_derived = old_is_derived;
    ctx.pop_private_scope();
    // Issue #562: restore prior super-ident slot.
    ctx.current_class_super_ident = old_super_ident;

    // Phase 4.1: register method + getter return types — see the parallel
    // site in lower_class_decl.
    for m in &methods {
        if !matches!(m.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.to_string(),
                m.name.clone(),
                m.return_type.clone(),
            );
        }
    }
    for (prop_name, g) in &getters {
        if !matches!(g.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.to_string(),
                prop_name.clone(),
                g.return_type.clone(),
            );
        }
    }

    // Mirror `lower_class_decl`: register the union of this class's accessor
    // names (own get/set, including private and the parent chain) so the
    // assignment recogniser in `expr_assign.rs` treats `C.prototype.<accessor>
    // = v` as a setter INVOCATION instead of a prototype-method monkey-patch.
    // `lower_class_decl` registers these for class declarations; without the
    // parallel call here, a class EXPRESSION's instance setters (e.g.
    // `var C = class { set ''(p){…} }; C.prototype[''] = v`) were silently
    // dropped to `RegisterPrototypeMethod`. Test262 accessor-name-inst setters.
    {
        let mut accessor_names = runtime_instance_accessor_names(&class.body);
        if let Some(ref parent_name) = extends_name {
            if let Some(parent_accessors) = ctx.lookup_class_accessor_names(parent_name) {
                accessor_names.extend_from(parent_accessors);
            }
        }
        ctx.register_class_accessor_names(name.to_string(), accessor_names);
    }

    // Issue #740: synthesize __perry_cap_* capture machinery for class
    // expressions that reference enclosing-fn locals (e.g. `const Inner =
    // class { _tag = tag }` inside `function makeFactory(tag)`). Without
    // this, anon class expressions silently dropped captures while named
    // class declarations had the machinery via `lower_class_decl`. See
    // the helper's doc comment for the full description.
    synthesize_class_captures(
        ctx,
        name,
        capture_parent_name.as_deref(),
        extends.is_some()
            || extends_name.is_some()
            || native_extends.is_some()
            || extends_expr.is_some(),
        &mut fields,
        &mut methods,
        &mut getters,
        &mut setters,
        &mut computed_members,
        &mut constructor,
        &mut static_methods,
        &static_accessor_fn_ids,
    );

    Ok(Class {
        id: class_id,
        name: name.to_string(),
        type_params,
        extends,
        extends_name,
        native_extends,
        extends_expr,
        heritage_lexically_shadowed,
        fields,
        constructor,
        methods,
        getters,
        setters,
        static_accessor_names,
        static_accessor_fn_ids,
        static_fields,
        static_methods,
        computed_members,
        decorators: lower_decorators(ctx, &class.decorators),
        is_exported,
        aliases: Vec::new(),
        // Declared inside a function body / non-module block → its static-field
        // initializers must run on class evaluation, not at module init.
        is_nested: ctx.scope_depth > 0 || ctx.inside_block_scope > 0,
        alloc_width_hint: 0,
        specialized_from: None,
    })
}
