use anyhow::Result;
use perry_types::{FuncId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

/// The four classic node:stream base-class names (`Readable`/`Writable`/
/// `Duplex`/`Transform`). When a class extends a parent with one of these
/// textual names, perry routes `super()` to the native `js_node_stream_*`
/// shim (which installs the native stream surface but never sets the
/// `_readableState`/`_writableState`/`_transformState` objects). That is only
/// correct when the name actually resolves to the `node:stream` builtin —
/// i.e. it was imported from `stream`/`node:stream`, in which case the import
/// machinery registered it as the `stream` native module
/// (`register_native_module(binding, "stream", Some(name))`, see
/// `var_decl_sources::register_destructured_stream_ctors` and the import
/// arms in `lower/module_decl.rs`).
///
/// The same textual name can instead be a userland binding from a
/// stream-shim npm package — `const { Transform } =
/// require('readable-stream')` in winston's `logger.js`, where
/// `class Logger extends Transform` then reads `this._readableState.pipes`
/// directly. Such a binding never registers as the `stream` native module
/// (readable-stream is not a node builtin), so the package's real constructor
/// must run instead. Returning `false` here lets the unknown-Ident arm
/// capture the parent as `extends_expr` and route `super()` through the
/// dynamic-parent path (`js_register_class_parent_dynamic` + the
/// `js_fetch_or_value_super` dispatch), which runs the real `Transform`
/// function body on the subclass instance.
fn is_genuine_node_stream_parent(ctx: &LoweringContext, name: &str) -> bool {
    if !matches!(name, "Readable" | "Writable" | "Duplex" | "Transform") {
        return false;
    }
    // Only the genuine `node:stream` builtin import registers these names as
    // the `stream` native module. Anything else (a userland require/import of
    // a stream-shim package, or an unbound reference) is not the builtin.
    matches!(ctx.lookup_native_module(name), Some(("stream", _)))
}

use super::*;

fn generic_computed_member_key<'a>(
    ctx: &LoweringContext,
    method: &'a ast::ClassMethod,
) -> Option<&'a ast::ComputedPropName> {
    let ast::PropName::Computed(computed) = &method.key else {
        return None;
    };
    if is_symbol_iterator_key(&computed.expr)
        || is_inspect_custom_key(ctx, &computed.expr)
        || symbol_well_known_key(&computed.expr).is_some()
    {
        return None;
    }
    Some(computed)
}

fn computed_member_name(kind: ast::MethodKind, computed: &ast::ComputedPropName) -> String {
    let base = match kind {
        ast::MethodKind::Method => "__computed_method",
        ast::MethodKind::Getter => "__computed_getter",
        ast::MethodKind::Setter => "__computed_setter",
    };
    format!("{}_{}_{}", base, computed.span.lo.0, computed.span.hi.0)
}

fn with_static_member_context<T>(
    ctx: &mut LoweringContext,
    is_static: bool,
    f: impl FnOnce(&mut LoweringContext) -> Result<T>,
) -> Result<T> {
    let old = ctx.current_class_member_is_static;
    ctx.current_class_member_is_static = is_static;
    let result = f(ctx);
    ctx.current_class_member_is_static = old;
    result
}

fn runtime_instance_accessor_names(members: &[ast::ClassMember]) -> crate::ClassAccessorNames {
    let mut accessor_names = crate::ClassAccessorNames::default();

    for member in members {
        match member {
            ast::ClassMember::Method(m)
                if !m.is_static
                    && m.function.body.is_some()
                    && matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
            {
                let key = match &m.key {
                    ast::PropName::Ident(i) => i.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    ast::PropName::Num(n) => crate::lower::number_to_js_key(n.value),
                    // #5592: a computed accessor key (`get [expr]()` /
                    // `set [expr](v)`) isn't statically known. Mark the class so
                    // `obj.prototype.<x> = v` writes route through the generic
                    // setter-invoking path rather than a name-keyed prototype
                    // monkey-patch.
                    ast::PropName::Computed(_) => {
                        accessor_names.has_computed = true;
                        continue;
                    }
                    _ => continue,
                };
                match m.kind {
                    ast::MethodKind::Getter => {
                        accessor_names.insert_getter(key);
                    }
                    ast::MethodKind::Setter => {
                        accessor_names.insert_setter(key);
                    }
                    _ => {}
                }
            }
            ast::ClassMember::PrivateMethod(m)
                if !m.is_static
                    && m.function.body.is_some()
                    && matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
            {
                let key = format!("#{}", m.key.name);
                match m.kind {
                    ast::MethodKind::Getter => {
                        accessor_names.insert_getter(key);
                    }
                    ast::MethodKind::Setter => {
                        accessor_names.insert_setter(key);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    accessor_names
}

fn lower_generic_computed_class_member(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    computed: &ast::ComputedPropName,
) -> Result<ClassComputedMember> {
    let key_expr = lower_expr(ctx, &computed.expr)?;
    let function_name = computed_member_name(method.kind, computed);
    let (kind, function) = match method.kind {
        ast::MethodKind::Method => (
            ClassComputedMemberKind::Method,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_class_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Getter => (
            ClassComputedMemberKind::Getter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_getter_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Setter => (
            ClassComputedMemberKind::Setter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_setter_method_with_name(ctx, method, function_name)
            })?,
        ),
    };
    Ok(ClassComputedMember {
        key_expr,
        function,
        is_static: method.is_static,
        kind,
    })
}

fn noncomputed_member_registration_name(
    kind: ast::MethodKind,
    method: &ast::ClassMethod,
) -> String {
    let base = match kind {
        ast::MethodKind::Method => "__computed_method_named",
        ast::MethodKind::Getter => "__computed_getter_named",
        ast::MethodKind::Setter => "__computed_setter_named",
    };
    format!("{}_{}_{}", base, method.span.lo.0, method.span.hi.0)
}

fn lower_noncomputed_class_member_registration(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    prop_name: &str,
) -> Result<ClassComputedMember> {
    let function_name = noncomputed_member_registration_name(method.kind, method);
    let (kind, function) = match method.kind {
        ast::MethodKind::Method => (
            ClassComputedMemberKind::Method,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_class_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Getter => (
            ClassComputedMemberKind::Getter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_getter_method_with_name(ctx, method, function_name)
            })?,
        ),
        ast::MethodKind::Setter => (
            ClassComputedMemberKind::Setter,
            with_static_member_context(ctx, method.is_static, |ctx| {
                lower_setter_method_with_name(ctx, method, function_name)
            })?,
        ),
    };
    Ok(ClassComputedMember {
        key_expr: Expr::String(prop_name.to_string()),
        function,
        is_static: method.is_static,
        kind,
    })
}

/// Lower a generator `*[Symbol.iterator]()` class method (already lowered into
/// `func`, named `@@iterator`) into the runtime `@@iterator` vtable entry.
///
/// The body is lifted to a top-level `__perry_iter_<class>` generator with
/// `this` as an explicit first parameter — the generator transform (which only
/// visits `module.functions`) then rewrites it to the `{next, return, throw}`
/// closure triple, and the syntactic `for…of` fast path dispatches to it
/// directly via `iterator_func_for_class`.
///
/// But every *runtime*-dispatched iterator consumer (spread `[...x]`,
/// `Math.max(...x)`, destructuring, `x[Symbol.iterator]()`, `Array.from`)
/// resolves `@@iterator` through the class registry instead. So this also
/// returns a synthetic NON-generator `@@iterator` wrapper method that forwards
/// to the lifted generator (`return __perry_iter_X(this)`) for the caller to
/// append to the instance vtable. Without it the class carries no `@@iterator`
/// for those consumers to find and they throw "value is not iterable" (#5128).
/// (The runtime maps the well-known `Symbol.iterator` to this `@@iterator`
/// method name in `js_object_get_symbol_property`.)
///
/// Shared by `lower_class_decl` and `lower_class_from_ast` so class
/// declarations and class expressions behave identically.
fn synthesize_symbol_iterator_wrapper(
    ctx: &mut LoweringContext,
    class_name: &str,
    func: &mut Function,
) -> Function {
    let this_id = ctx.fresh_local();
    let mut new_params = Vec::with_capacity(func.params.len() + 1);
    new_params.push(Param {
        id: this_id,
        name: "this".to_string(),
        ty: Type::Named(class_name.to_string()),
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    });
    new_params.append(&mut func.params);

    let mut body = std::mem::take(&mut func.body);
    crate::analysis::replace_this_in_stmts(&mut body, this_id);

    let top_fn_id = ctx.fresh_func();
    let top_fn = Function {
        id: top_fn_id,
        name: format!("__perry_iter_{}", class_name),
        type_params: Vec::new(),
        params: new_params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: true,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    };
    ctx.pending_functions.push(top_fn);
    ctx.iterator_func_for_class
        .insert(class_name.to_string(), top_fn_id);

    Function {
        id: ctx.fresh_func(),
        name: "@@iterator".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(top_fn_id)),
            args: vec![Expr::This],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

pub fn lower_class_decl(
    ctx: &mut LoweringContext,
    class_decl: &ast::ClassDecl,
    is_exported: bool,
) -> Result<Class> {
    // Resolve through any active scope-local rename so a disambiguated
    // duplicate class registers (and self-references) under its unique name.
    let name = ctx.resolve_class_name(class_decl.ident.sym.as_str());
    validate_legacy_decorator_surface(&class_decl.class, &name)?;
    validate_class_element_early_errors(&class_decl.class, &name)?;
    let class_id = match ctx.lookup_class(&name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.clone(), id);
            id
        }
    };

    // Set current class for arrow function `this` capture tracking
    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.clone());
    let old_is_derived = ctx.current_class_is_derived;
    ctx.current_class_is_derived = class_decl.class.super_class.is_some();

    // Push the private-name scope for this class body so `obj.#name` accesses
    // brand-check against the declaring class and reject illegal read/write
    // operations. Popped at the matching restore below.
    ctx.push_private_scope(super::build_private_scope(&class_decl.class, &name));

    // Issue #562: track the parent class identifier so the `super({...})`
    // pre-scan in expr_call.rs can register the controller param as a
    // readable_stream instance for stream subclass constructors. Set
    // here BEFORE constructor lowering so the body lowering picks it up.
    let old_super_ident = ctx.current_class_super_ident.take();
    ctx.current_class_super_ident = match class_decl.class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => Some(ident.sym.to_string()),
        _ => None,
    };

    // Extract type parameters from generic class declaration (e.g., class Box<T>)
    let type_params = class_decl
        .class
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type parameter scope for resolving T, U, etc. in member types
    ctx.enter_type_param_scope(&type_params);

    // #5437: does the parent Ident resolve to an in-scope lexical local that
    // shadows a same-named native/built-in parent? Computed here (same `ctx`
    // scope state the heritage routing below uses, before the body lowers any
    // new locals) so codegen can prefer the dynamic local over a NAME-keyed
    // built-in special case (Error/Request/Response/Event/CustomEvent/streams).
    let heritage_lexically_shadowed = match class_decl.class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => {
            let n = ident.sym.to_string();
            !ctx.class_renames.contains_key(&n) && ctx.locals.lookup(&n).is_some()
        }
        _ => false,
    };

    // Handle extends clause
    let (extends, extends_name, native_extends, extends_expr) = if let Some(ref super_class) =
        class_decl.class.super_class
    {
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            // First check if it's a native module class
            let native_parent = match parent_name.as_str() {
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
                // Issue #562: user classes extending the Web Streams
                // base classes get a runtime-side subclass-init shim
                // wired through `Expr::SuperCall` (codegen). The
                // `extends_name` is also retained so the existing
                // `native_extends.is_some()` branch below still
                // populates it for the inheritance walks elsewhere
                // (vtable, hasOwn, etc.) that key on the parent name.
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
                // #1545: classic node:stream base classes. Recognising them
                // as native parents (rather than letting the unknown-Ident
                // arm capture `extends_expr`) avoids the dynamic
                // parent-registration throw ("Class extends value is not a
                // constructor") — a node:stream export is a callable but not
                // a registered class constructor. The `super(opts)` codegen
                // arm (`lower_node_stream_super_init`) and the runtime
                // `js_node_stream_*_subclass_init` helpers, which install the
                // native stream methods directly onto `this`, were already
                // built; this is the missing HIR wiring. Keep in lockstep
                // with the parallel arm in `lower_class_from_ast` below.
                //
                // Gate the classic node:stream names on `is_genuine_node_stream_parent`
                // so a userland stream-shim binding (readable-stream's
                // `Transform`, winston) falls through to the dynamic
                // `extends_expr` parent path and runs its real constructor.
                "Readable" | "Writable" | "Duplex" | "Transform"
                    if is_genuine_node_stream_parent(ctx, &parent_name) =>
                {
                    Some(("node_stream".to_string(), parent_name.clone()))
                }
                _ => None,
            };
            // A lexical local shadowing the parent name must win over the native
            // parent — the in-scope local IS the real parent value. Check it
            // BEFORE `native_parent` so `const EventEmitter = …; class X extends
            // EventEmitter {}` binds to the local via the dynamic `extends_expr`
            // path, not the native `events` parent. ESM imports are not in
            // `ctx.locals`, so genuine native subclassing is unchanged. Mirrors
            // the class-expression arm below.
            let locally_shadowed = !ctx.class_renames.contains_key(&parent_name)
                && ctx.locals.lookup(&parent_name).is_some();
            if native_parent.is_some() && !locally_shadowed {
                // Keep `extends_name` populated alongside `native_extends`
                // so SuperCall codegen + downstream chain walks still
                // see the parent name (mirrors how stream-class
                // dispatch resolves through the existing extends_name
                // path while the native_extends carries the (module,
                // class) tag for the runtime shim).
                (None, Some(parent_name), native_parent, None)
            } else if locally_shadowed {
                // Lexical local shadow → dynamic parent via `extends_expr` (the
                // in-scope local value), invoked by `super()` through
                // `js_fetch_or_value_super`. See the class-expression arm below
                // for the full rationale (Next.js p-queue `PQueue`).
                match lower_expr(ctx, super_class) {
                    Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (None, Some(parent_name), None, None),
                }
            } else {
                // #5437 (Next.js NodeNextRequest cross-module heritage): a
                // minified bundle declares the SAME class name in several
                // turbopack factory closures (`class f{...}` appears 3× in one
                // chunk). Phase-1.5 disambiguates the duplicate sibling by
                // renaming it (`f` -> `f$0`) in this body's scope, and the
                // class registers under that unique name — but the child's
                // `extends f` was binding to the RAW name, which `lookup_class`
                // resolves to the FIRST (wrong) `f`. The parent-chain walk then
                // pulls the wrong class's fields into `packed_keys`, dropping
                // the real parent's `method`/`url`/`body` (NodeNextRequest's
                // `e.url` read undefined -> `Invalid URL` 500 on dynamic page
                // routes). Resolve the parent name through the active
                // scope-local renames so heritage binds to the same disambiguated
                // class the parent decl registered under. Identity for
                // non-colliding names, so unaffected classes keep their parent.
                let parent_name = ctx.resolve_class_name(&parent_name);
                let parent_cid = ctx.lookup_class(&parent_name);
                if parent_cid.is_none() {
                    // Issue #711 part 2: the Ident doesn't resolve to
                    // any known class. The common case is Effect's
                    // `const Base = (function() { function Base(){}; Base.prototype = X; return Base })()` pattern —
                    // `Base` is a function value with a prototype
                    // object attached via `js_set_function_prototype`.
                    // Capture the Ident as `extends_expr` so the
                    // dynamic parent-registration helper can resolve
                    // it through `function_class_id` at runtime.
                    // `extends_name` stays populated for the rare
                    // cases where downstream code paths key on the
                    // textual parent name (super-call codegen, etc.)
                    // but extends_expr takes precedence on the
                    // method-dispatch path.
                    match lower_expr(ctx, super_class) {
                        Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (None, Some(parent_name), None, None),
                    }
                } else {
                    // Always capture the parent name for imported classes that may not have a ClassId
                    (parent_cid, Some(parent_name), None, None)
                }
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            // Handle member expression like ethers.JsonRpcProvider or module.ClassName
            let parent_name = extract_member_class_name(member);
            // Issue #4908: `extract_member_class_name` returns only the
            // trailing property (`http.Agent` -> "Agent"). When that equals
            // the subclass's OWN name (`class Agent extends http.Agent`), the
            // bare-name resolution is bogus: the subclass registered its own
            // name above (so `lookup_class` returns the class itself) and
            // both `extends` and `extends_name` would self-reference. A
            // self-link sends every codegen parent-chain walk into an
            // infinite loop — the four node:http `class Agent extends
            // http.Agent` tests OOM-crashed codegen. Leave the class
            // parentless (all None), matching how a non-colliding native
            // member base (`class Foo extends http.Agent`) already behaves:
            // `extends_name` there resolves to no known class, so the class
            // is effectively parentless and constructs cleanly. We do NOT
            // route through the dynamic `extends_expr` path here — that
            // turns the class derived and demands a runtime super() into the
            // native base, which fails ("Class extends value is not a
            // constructor" / "Must call super constructor"). Native member
            // base inheritance (real `instanceof` / `super.method` dispatch)
            // is unimplemented for the member-expression case generally;
            // this keeps the colliding-name case on par with the rest.
            if parent_name == name {
                (None, None, None, None)
            } else if parent_name == "default" {
                // `class X extends _mod.default` — the interop ESM
                // default-export-class pattern (Next.js `NextNodeServer
                // extends base-server`'s default `Server`). The trailing
                // property `default` never resolves through `lookup_class`,
                // and a `.default` export is always a real user/registered
                // class — never a native-module member like `http.Agent`
                // (which inherits via a *named* property and is handled by
                // the colliding-name / parentless branches). Route through
                // the dynamic `extends_expr` path so `super(opts)`
                // re-evaluates the alias at construction time and runs the
                // base constructor, and the decl-time
                // `RegisterClassParentDynamic` wires the real parent edge
                // (inherited methods / `instanceof`). The companion hoist
                // guard in `extract_top_level_class_decls` keeps this class
                // inside the IIFE so the require alias is assigned before the
                // registration runs.
                match lower_expr(ctx, super_class) {
                    Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (None, Some(parent_name), None, None),
                }
            } else {
                // A NAMED cross-module member-extends (`class NodeNextRequest
                // extends _index.BaseNextRequest`). The static `extends_name`
                // path requires the parent in codegen's class table, which a
                // cross-module parent often is NOT — and `ctx.lookup_class` is
                // module-order-dependent (the parent module may not be lowered
                // yet) — so `super(...)` became a no-op and the parent ctor never
                // ran (Next.js `BaseNextRequest`'s ctor sets `this.url`/
                // `this.method` → "Invariant: url can not be undefined"). Route
                // through the dynamic `extends_expr` path UNCONDITIONALLY, exactly
                // like the `.default` arm (wall 38) and the unknown-Ident arm
                // below: the decl-time `RegisterClassParentDynamic` records the
                // parent value and `super()` runs the parent ctor at runtime via
                // `js_fetch_or_value_super`, which already tolerates native /
                // closure / class-ref / builtin parents (wall 38/42 hardening).
                // Keep the (possibly-None) static `extends` link + `extends_name`
                // for inherited-method / `instanceof` dispatch when resolvable.
                // The colliding-name native case (`class Agent extends http.Agent`)
                // is handled by the `parent_name == name` arm above. (Refs #488
                // drizzle-sqlite for the original cross-module link.)
                let resolved = ctx.lookup_class(&parent_name);
                match lower_expr(ctx, super_class) {
                    Ok(expr) => (resolved, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (resolved, Some(parent_name), None, None),
                }
            }
        } else {
            // Issue #711: `class X extends fn(...)` / `class X extends
            // new Foo(...)` etc. The super-class expression isn't
            // statically resolvable to a known class. Lower the
            // expression so codegen can evaluate it at the class
            // declaration site and call
            // `js_register_class_parent_dynamic` to wire the parent
            // edge into CLASS_REGISTRY at runtime. Both `extends` and
            // `extends_name` stay None — the parent class_id is only
            // known once the expression evaluates. Lowering errors
            // here are non-fatal: fall back to a parentless class so
            // the rest of the program still compiles (the
            // method-dispatch catch-all in object.rs surfaces the
            // missing-method case clearly enough).
            match lower_expr(ctx, super_class) {
                Ok(expr) => (None, None, None, Some(Box::new(expr))),
                Err(_) => (None, None, None, None),
            }
        }
    } else {
        (None, None, None, None)
    };

    // First pass: collect static field/method names for early registration
    // This allows static method bodies to reference static fields
    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class_decl.class.body {
        match member {
            // Static accessors (`static get foo()`) are not callable static
            // methods: `C.foo(...)` must read the accessor and call its result.
            // Excluding getter/setter kinds keeps `has_static_method` from
            // hijacking the call into a non-existent StaticMethodCall. Refs
            // test262 language/arguments-object cls-*-static-* getter calls.
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
                // Register as "#name" so WithPrivateStatic.#helper()
                // call-site lookup via has_static_method() succeeds.
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

    // Register static members early so method bodies can reference them
    ctx.register_class_statics(name.clone(), static_field_names, static_method_names);

    // Issue #302: also collect instance field TYPES early so method bodies'
    // `for (... of this.someField)` lowering can detect Map/Set field types
    // BEFORE method bodies are lowered. The full `fields` Vec gets populated
    // during the next pass starting at line 672 (with init exprs etc.); for
    // type-name lookup we only need (name, declared type) which is cheap to
    // pluck from `prop.type_ann`. Registered again at end-of-class
    // (line ~1058) once `fields` is complete in case any field types got
    // refined during body lowering.
    //
    // Issue #305 (re-repro on v0.5.415): fields without an explicit
    // annotation but WITH an initializer like `private map = new Map<K,V>()`
    // also need their generic type registered, otherwise `for-of this.map`
    // and `const m = this.map; for-of m` (whose type inference consults this
    // registry via lower_types::infer_type_from_expr's `this.<field>` arm)
    // both fall off the Map fast path and the loop body never executes.
    // Falls back to `infer_type_from_expr` on the AST initializer when the
    // annotation is absent — this is the same routine used elsewhere for
    // `let m = new Map<K,V>()`, so the two shapes now agree.
    let mut early_field_types: Vec<(String, Type)> = Vec::new();
    for member in &class_decl.class.body {
        if let ast::ClassMember::ClassProp(prop) = member {
            if prop.is_static {
                continue;
            }
            let field_name = match &prop.key {
                ast::PropName::Ident(i) => i.sym.to_string(),
                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                _ => continue,
            };
            let ty = match prop.type_ann.as_ref() {
                Some(ann) => extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)),
                None => prop
                    .value
                    .as_ref()
                    .map(|v| infer_type_from_expr(v, ctx))
                    .unwrap_or(Type::Any),
            };
            early_field_types.push((field_name, ty));
        }
    }
    // TypeScript constructor parameter properties (`constructor(readonly
    // stack: Node[])`) are class fields too, but they live on the
    // constructor's param list rather than as `ClassProp` members — so the
    // ClassProp loop above misses them. Without registering their declared
    // types here, `const s = this.stack` inside a method infers `Any` (the
    // `this.<field>` arm of `infer_type_from_expr` consults this registry),
    // which knocks the subsequent `s[i]` element read off the array fast
    // path. That mis-lowered `effect`'s `RedBlackTreeIterator` (whose
    // `readonly stack: Array<Node<K,V>>` is a param-prop): a local alias
    // `const stack = this.stack; stack[len-1]` returned garbage nodes, so
    // in-order traversal lost the last node and SortedSet iteration came
    // back short (#321). Mirror the param-prop field detection that runs
    // later (when `fields` is built) so the early registry agrees.
    for member in &class_decl.class.body {
        if let ast::ClassMember::Constructor(ctor) = member {
            for param in &ctor.params {
                if let ast::ParamOrTsParamProp::TsParamProp(ts_prop) = param {
                    let (param_name, param_type) = match &ts_prop.param {
                        ast::TsParamPropParam::Ident(ident) => {
                            let pname = ident.id.sym.to_string();
                            let ty = ident
                                .type_ann
                                .as_ref()
                                .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                                .unwrap_or(Type::Any);
                            (pname, ty)
                        }
                        ast::TsParamPropParam::Assign(assign) => {
                            let pname = get_pat_name(&assign.left).unwrap_or_default();
                            let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                            (pname, ty)
                        }
                    };
                    if !param_name.is_empty()
                        && !early_field_types.iter().any(|(n, _)| *n == param_name)
                    {
                        early_field_types.push((param_name, param_type));
                    }
                }
            }
        }
    }
    ctx.register_class_field_types(name.clone(), early_field_types);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();
    let mut static_accessor_names: Vec<String> = Vec::new();
    let mut static_accessor_fn_ids: Vec<FuncId> = Vec::new();
    let mut computed_members = Vec::new();
    let mut seen_generic_computed_member = false;

    // Second pass: actually lower the class members
    for member in &class_decl.class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, &name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                if let Some(computed) = generic_computed_member_key(ctx, method) {
                    computed_members
                        .push(lower_generic_computed_class_member(ctx, method, computed)?);
                    seen_generic_computed_member = true;
                    continue;
                }
                // Get the property name for getters/setters. Computed
                // keys are accepted for `[Symbol.iterator]` (registered
                // under `@@iterator`), and for `[Symbol.hasInstance]` /
                // `[Symbol.toStringTag]` (lifted to top-level functions
                // with a `__perry_wk_<hook>_<class>` prefix so the LLVM
                // backend's `init_static_fields` picks them up and
                // registers them with the runtime).
                let (prop_name, can_source_order_register) = match &method.key {
                    ast::PropName::Ident(ident) => (ident.sym.to_string(), true),
                    ast::PropName::Str(s) => (s.value.as_str().unwrap_or("").to_string(), true),
                    // Numeric-literal member names (`get 0()`, `set 1.5(v)`,
                    // `42() {}`) are valid class element keys — their property
                    // key is the canonical ToString of the numeric value, the
                    // same conversion object literals use (`{ 0: ... }`).
                    // Without this arm they fell through `_ => continue` and the
                    // method/accessor was silently dropped, so `C.prototype[0]`
                    // read `undefined` (Test262 accessor-name-inst/literal-numeric-*).
                    ast::PropName::Num(n) => (crate::lower::number_to_js_key(n.value), true),
                    ast::PropName::Computed(computed) => {
                        if is_symbol_iterator_key(&computed.expr) {
                            ("@@iterator".to_string(), false)
                        } else if is_inspect_custom_key(ctx, &computed.expr)
                            && !method.is_static
                            && matches!(method.kind, ast::MethodKind::Method)
                        {
                            // `[util.inspect.custom]() {}` on a class — rename
                            // to a stable string key so `js_register_class_method`
                            // picks it up. `format_object_as_json` looks up
                            // this name on the object's vtable when there is no
                            // per-instance entry. Refs #1248.
                            ("__perry_inspect_custom__".to_string(), false)
                        } else if let Some(wk) = symbol_well_known_key(&computed.expr) {
                            // hasInstance (static method): lift the method
                            // body to a top-level function named
                            // `__perry_wk_hasinstance_<class>`. Signature:
                            // `(value: f64) -> f64` — no `this`.
                            if wk == "hasInstance"
                                && method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                let mut func =
                                    with_static_member_context(ctx, method.is_static, |ctx| {
                                        lower_class_method(ctx, method)
                                    })?;
                                func.name = format!("__perry_wk_hasinstance_{}", name);
                                ctx.pending_functions.push(func);
                                continue;
                            }
                            // toStringTag (instance getter): lift the
                            // getter body to a top-level function named
                            // `__perry_wk_tostringtag_<class>`. Signature:
                            // `(this: f64) -> f64` — getter takes `this`
                            // as an explicit first parameter and returns
                            // a string.
                            if wk == "toStringTag"
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Getter)
                            {
                                let getter =
                                    with_static_member_context(ctx, method.is_static, |ctx| {
                                        lower_getter_method(ctx, method)
                                    })?;
                                // Inject a `this` parameter at position 0 and rewrite
                                // any `Expr::This` in the body to `LocalGet(this_id)`.
                                let this_id = ctx.fresh_local();
                                let mut new_params = Vec::with_capacity(getter.params.len() + 1);
                                new_params.push(Param {
                                    id: this_id,
                                    name: "this".to_string(),
                                    ty: Type::Named(name.clone()),
                                    default: None,
                                    decorators: Vec::new(),
                                    is_rest: false,
                                    arguments_object: None,
                                });
                                new_params.extend(getter.params);
                                let mut body = getter.body;
                                crate::analysis::replace_this_in_stmts(&mut body, this_id);
                                let top_fn = Function {
                                    id: ctx.fresh_func(),
                                    name: format!("__perry_wk_tostringtag_{}", name),
                                    type_params: Vec::new(),
                                    params: new_params,
                                    return_type: Type::Any,
                                    body,
                                    is_async: false,
                                    is_generator: false,
                                    is_strict: true,
                                    was_plain_async: false,
                                    was_unrolled: false,
                                    is_exported: false,
                                    captures: Vec::new(),
                                    decorators: Vec::new(),
                                };
                                ctx.pending_functions.push(top_fn);
                                continue;
                            }
                            // `[Symbol.dispose]()` / `[Symbol.asyncDispose]()`:
                            // ES2024 explicit-resource-management dispose hooks.
                            // Rename the method to a stable string-keyed name so
                            // the using-block desugarer can call it via plain
                            // method dispatch (`obj.__perry_dispose__()` /
                            // `obj.__perry_async_dispose__()`). Falls through to
                            // the regular method-pushing path below with the
                            // renamed key.
                            if (wk == "dispose" || wk == "asyncDispose")
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                if wk == "asyncDispose" {
                                    ("__perry_async_dispose__".to_string(), false)
                                } else {
                                    ("__perry_dispose__".to_string(), false)
                                }
                            } else if wk == "asyncIterator"
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                // #1838 follow-up: `[Symbol.asyncIterator]() {}`
                                // on a class — register under `@@asyncIterator`
                                // so the symbol resolver in `runtime/src/symbol.rs`
                                // (`well_known_symbol_method_key`) binds it as
                                // `instance[Symbol.asyncIterator]`. Mirrors the
                                // `@@iterator` path; for-await over a class
                                // instance picks the same vtable entry.
                                ("@@asyncIterator".to_string(), false)
                            } else if wk == "toPrimitive"
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                // #2374: `[Symbol.toPrimitive](hint) {}` on a
                                // class — register under `@@toPrimitive` so the
                                // symbol resolver in `runtime/src/symbol.rs`
                                // (`well_known_symbol_method_key`) binds it as
                                // `instance[Symbol.toPrimitive]`. The runtime's
                                // ToPrimitive (`js_to_primitive`, consulted by
                                // unary `+` numeric coercion and template/`String()`
                                // string coercion) then invokes it with the
                                // appropriate hint before falling back to
                                // `valueOf`/`toString`. Mirrors the `@@iterator`
                                // / `@@asyncIterator` path. Pre-fix the method
                                // was dropped here, so class instances coerced to
                                // `NaN` / `[object Object]`.
                                ("@@toPrimitive".to_string(), false)
                            } else {
                                // Other well-known on a class: not yet
                                // implemented, skip.
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                match method.kind {
                    ast::MethodKind::Getter => {
                        // Getter: no parameters, returns a value
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_getter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        // Setter: takes one parameter
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_setter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        setters.push((prop_name, func));
                    }
                    ast::MethodKind::Method => {
                        let mut func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_class_method(ctx, method)
                        })?;
                        // Issue #212 fixed the broader class-method-captures-
                        // outer-fn-local codegen gap, so the dispose family no
                        // longer needs a silent-drop fallback — the same
                        // hidden-field rewrite that lets `log() { captured.push(...) }`
                        // work also lets `[Symbol.dispose]() { disposed.push(...) }`
                        // work. The pre-fix gate at this site (`scope_depth > 0
                        // && method_body_captures_outer(...)` → `continue`) was
                        // removed in v0.5.319. See the v0.5.317 entry for the
                        // history and `test_issue_154_using_dispose.ts` for the
                        // regression test.
                        // `*[Symbol.iterator]()` — lift to a top-level generator
                        // and register a synthetic `@@iterator` wrapper so both
                        // the `for…of` fast path and runtime-dispatched iterator
                        // consumers work (#5128). See the helper for details.
                        if prop_name == "@@iterator" && func.is_generator && !method.is_static {
                            let wrapper = synthesize_symbol_iterator_wrapper(ctx, &name, &mut func);
                            methods.push(wrapper);
                            continue;
                        }
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
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
                if prop.declare {
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
                // Skip TypeScript overload declarations (no body)
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
                        // Store under "#name" so PropertyGet on "#name"
                        // can hit the getter registry (which keys on
                        // the property name, not `get_#name`).
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_getter(ctx, method)?;
                        // A STATIC private accessor must register on the
                        // class's static-accessor side (mirroring the public
                        // static getter/setter arms above) so `this.#f` with
                        // a class-ref receiver dispatches it. Pre-fix it only
                        // landed in the instance getter registry and the
                        // static read returned undefined (test262
                        // static-private-getter*).
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_setter(ctx, method)?;
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        setters.push((prop_name, func));
                    }
                }
            }
            ast::ClassMember::StaticBlock(block) => {
                // `static { ... }` — lower the body and attach it as
                // a synthetic static method whose name is
                // `__perry_static_init_N`. `codegen.rs :: init_static_fields`
                // later recognizes the prefix and emits a call to each
                // such method right after static field init, so they
                // run once at module startup.
                let scope_mark = ctx.enter_scope();
                let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
                ctx.in_nonarrow_fn = true;
                let body = lower_block_stmt(ctx, &block.body)?;
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

    // Detect fields from TypeScript parameter properties (e.g., constructor(public name: string)).
    // SWC represents these as TsParamProp in the AST. They must be registered as class fields
    // so that `this.name` access in methods can find them by field index.
    {
        let declared_field_names: std::collections::HashSet<String> =
            fields.iter().map(|f| f.name.clone()).collect();
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    if let ast::ParamOrTsParamProp::TsParamProp(ts_prop) = param {
                        let (param_name, param_type) = match &ts_prop.param {
                            ast::TsParamPropParam::Ident(ident) => {
                                let pname = ident.id.sym.to_string();
                                let ty = ident
                                    .type_ann
                                    .as_ref()
                                    .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                (pname, ty)
                            }
                            ast::TsParamPropParam::Assign(assign) => {
                                let pname = get_pat_name(&assign.left).unwrap_or_default();
                                let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                                (pname, ty)
                            }
                        };
                        if !param_name.is_empty() && !declared_field_names.contains(&param_name) {
                            fields.push(ClassField {
                                name: param_name,
                                key_expr: None,
                                ty: param_type,
                                init: None,
                                is_private: false,
                                is_readonly: ts_prop.readonly,
                                decorators: lower_decorators(ctx, &ts_prop.decorators),
                            });
                        }
                    }
                }
            }
        }
    }

    // Detect fields from constructor body `this.xxx = ...` assignments.
    // JavaScript classes (e.g., transpiled from TypeScript) often don't have ClassProp
    // declarations; instead they assign to `this` in the constructor body.
    //
    // IMPORTANT: Also exclude fields inherited from parent classes. If the parent already
    // declares `kind` and the subclass writes `this.kind = ...`, the subclass must NOT
    // add `kind` as a new own field. Otherwise, codegen's resolve_class_fields later
    // merges parent and own indices and the subclass's shadow `kind` gets a different
    // offset from the parent's, leaving TWO `kind` slots that disagree at runtime.
    {
        // Collect inherited field names by walking the parent chain via the extends_name.
        // Previous lower_class_decl calls have registered each class's full (own+inherited)
        // field set, so a single lookup on the direct parent yields the complete chain.
        let mut inherited_field_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Some(ref parent_name) = extends_name {
            if let Some(parent_fields) = ctx.lookup_class_field_names(parent_name) {
                for f in parent_fields {
                    inherited_field_names.insert(f.clone());
                }
            }
        }

        // Issue #665 (sixth pass): collect own + inherited accessor (getter+setter)
        // property names. Real-world packages like rate-limiter-flexible
        // declare a `set points(v)` accessor AND write `this.points = opts.points`
        // from the constructor body. Pre-fix the bare-this scan below
        // mis-categorised `points` as an own data field, allocating an
        // inline slot that surfaced via `Object.keys` and shadowed the
        // accessor when a subclass instance's `.points` was read across
        // modules (the runtime's setter dispatch walks the class vtable
        // chain correctly, but the spurious own-data slot wins lookup).
        let mut accessor_names = runtime_instance_accessor_names(&class_decl.class.body);
        // Pull in accessor names from the parent chain. The parent's
        // registration stored the own+inherited union, so a single lookup
        // on the direct parent suffices.
        if let Some(ref parent_name) = extends_name {
            if let Some(parent_accessors) = ctx.lookup_class_accessor_names(parent_name) {
                accessor_names.extend_from(parent_accessors);
            }
        }

        // Own instance method names. A constructor `this.method = this.method.bind(this)`
        // (zod's `ZodType` ctor self-binds ~20 methods; React class components do the
        // same) is a METHOD OVERRIDE, not a new data field — the assignment creates a
        // runtime own property handled by the method-override dispatch path. Allocating
        // an inline field slot for it makes the codegen field branch shadow the method on
        // every read, so `this.method` reads the uninitialised slot (`undefined`) BEFORE
        // the assignment runs — exactly what made `this.parse.bind(this)` throw "Bind must
        // be called on a function" in zod. Mirrors the accessor exclusion (#665).
        let mut method_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for member in &class_decl.class.body {
            match member {
                ast::ClassMember::Method(m) if matches!(m.kind, ast::MethodKind::Method) => {
                    let key = match &m.key {
                        ast::PropName::Ident(i) => i.sym.to_string(),
                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        _ => continue,
                    };
                    method_names.insert(key);
                }
                ast::ClassMember::PrivateMethod(m) if matches!(m.kind, ast::MethodKind::Method) => {
                    method_names.insert(format!("#{}", m.key.name));
                }
                _ => {}
            }
        }

        let declared_field_names: std::collections::HashSet<String> =
            fields.iter().map(|f| f.name.clone()).collect();
        // Pull each top-level `this.<ident> = …` field name out of one ctor
        // statement-expression. Minified bundles (Next.js `BaseNextRequest`'s
        // `constructor(a,b,c){this.method=a,this.url=b,this.body=c}`) collapse
        // every ctor assignment into ONE comma-`Seq` expression-statement, so a
        // scan that only matched `Expr::Assign` detected ZERO fields — the
        // parent's `method`/`url`/`body` never entered `packed_keys`, leaving
        // the subclass instance allocated with too-few inline slots so the
        // captured-class shape prepends `__perry_cap_*` over the (missing) real
        // slots and `e.url` reads undefined ("Invalid URL" 500 on dynamic page
        // routes). Descend through `Seq` (and the `Paren`/`Assign`-result-chain
        // wrappers minifiers emit) so each comma-separated `this.x = …` is
        // recognised the same as a standalone assignment statement.
        fn collect_this_field_assigns(expr: &ast::Expr, out: &mut Vec<String>) {
            match expr {
                ast::Expr::Assign(assign) => {
                    // A chained assignment's RHS can itself be `this.x = …`
                    // (`this.a = this.b = v`): the inner `this.b = v` evaluates
                    // (and creates `b`'s slot) BEFORE the outer assignment to
                    // `this.a`, so collect the RHS first to keep Object.keys in
                    // the same insertion order Node produces (`b` then `a`).
                    collect_this_field_assigns(&assign.right, out);
                    if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(mem)) =
                        &assign.left
                    {
                        if let ast::Expr::This(_) = &*mem.obj {
                            if let ast::MemberProp::Ident(prop_ident) = &mem.prop {
                                out.push(prop_ident.sym.to_string());
                            }
                        }
                    }
                }
                ast::Expr::Seq(seq) => {
                    for e in &seq.exprs {
                        collect_this_field_assigns(e, out);
                    }
                }
                ast::Expr::Paren(p) => collect_this_field_assigns(&p.expr, out),
                _ => {}
            }
        }
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                if let Some(ref body) = ctor.body {
                    for stmt in &body.stmts {
                        if let ast::Stmt::Expr(expr_stmt) = stmt {
                            let mut names: Vec<String> = Vec::new();
                            collect_this_field_assigns(&expr_stmt.expr, &mut names);
                            for fname in names {
                                if !declared_field_names.contains(&fname)
                                    && !inherited_field_names.contains(&fname)
                                    && !accessor_names.contains_any(&fname)
                                    && !method_names.contains(&fname)
                                {
                                    fields.push(ClassField {
                                        name: fname,
                                        key_expr: None,
                                        ty: Type::Any,
                                        init: None,
                                        is_private: false,
                                        is_readonly: false,
                                        decorators: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        // Dedup fields: keep first occurrence of each name
        let mut seen = std::collections::HashSet::new();
        fields.retain(|f| seen.insert(f.name.clone()));

        // Register this class's complete field set (own + inherited) so subclasses that
        // extend it can see the full inheritance chain during their own lowering.
        let mut complete_field_names: Vec<String> = inherited_field_names.into_iter().collect();
        for f in &fields {
            if !complete_field_names.contains(&f.name) {
                complete_field_names.push(f.name.clone());
            }
        }
        ctx.register_class_field_names(name.clone(), complete_field_names);

        // Issue #665: register own+inherited accessor names so subclasses
        // lowered after this one can also skip them when scanning ctor
        // bodies. `accessor_names` already contains the getter/setter names
        // from the parent-chain lookup above.
        ctx.register_class_accessor_names(name.clone(), accessor_names);

        // Issue #302: also register field TYPES so the for-of arm can
        // detect `for (... of this.someMap)` patterns. Only own fields are
        // registered here; inherited field types fall through to whichever
        // ancestor class registered them (sub-class lookups walk via the
        // class hierarchy elsewhere if needed).
        let field_types: Vec<(String, Type)> = fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        ctx.register_class_field_types(name.clone(), field_types);
    }

    // `this` in a STATIC field initializer is the class constructor per
    // ClassDefinitionEvaluation. Substitute lexically — including inside
    // arrow / this-capturing closure BODIES (which compile from these very
    // exprs) — so every consumer (the inline init stmts at the class-decl
    // source position, init_static_fields_late) evaluates with the right
    // receiver. Without the in-place rewrite, a stmt-level clone substitution
    // desyncs the closure creation site from the compiled body (the body is
    // compiled from this original) and `static f = () => this` returned the
    // unpatched capture slot (test262 static-field-init-this-inside-arrow).
    for sf in &mut static_fields {
        if let Some(init) = &mut sf.init {
            crate::analysis::substitute_lexical_this_in_expr(init, &Expr::ClassRef(name.clone()));
        }
    }

    // Exit type parameter scope
    ctx.exit_type_param_scope();

    // Issue #562: stash native_extends so the `let x = new <subclass>()`
    // path in destructuring.rs can route the local through the parent
    // stream module. Done here (not at the call site) so the registry
    // lookup is always available regardless of declaration order.
    if let Some((module, class)) = native_extends.as_ref() {
        ctx.register_class_native_extends(name.clone(), module.clone(), class.clone());
    }

    // Restore previous current_class
    ctx.current_class = old_class;
    ctx.current_class_is_derived = old_is_derived;
    ctx.pop_private_scope();
    // Issue #562: restore the prior super-ident slot.
    ctx.current_class_super_ident = old_super_ident;

    // Issue #212: classes nested inside a function may have method bodies
    // that reference enclosing-fn locals. See `synthesize_class_captures`
    // for the full doc (extracted in #740 so anonymous class expressions
    // can use the same machinery).
    synthesize_class_captures(
        ctx,
        &name,
        extends_name.as_deref(),
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
    );

    // Phase 4.1: register each method's and getter's return type so
    // call-site inference (`infer_call_return_type`'s Member arm) can
    // resolve `obj.method()` when obj's type is Type::Named(name).
    // Feeds off Phase 4's body-based inference — any method without an
    // explicit annotation whose body returned a known type lands here too.
    for m in &methods {
        if !matches!(m.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.clone(),
                m.name.clone(),
                m.return_type.clone(),
            );
        }
    }
    for (prop_name, g) in &getters {
        if !matches!(g.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.clone(),
                prop_name.clone(),
                g.return_type.clone(),
            );
        }
    }

    Ok(Class {
        id: class_id,
        name,
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
        decorators: lower_decorators(ctx, &class_decl.class.decorators),
        is_exported,
        aliases: Vec::new(),
        // Declared inside a function body / non-module block → its static-field
        // initializers must run on class evaluation, not at module init.
        is_nested: ctx.scope_depth > 0 || ctx.inside_block_scope > 0,
    })
}

/// Lower a class expression (ast::Class) to HIR.
/// Used for anonymous class expressions like `new (class extends Command { ... })()`.
pub fn lower_class_from_ast(
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

    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.to_string());
    let old_is_derived = ctx.current_class_is_derived;
    ctx.current_class_is_derived = class.super_class.is_some();

    // Private-name scope for this class-expression body (see lower_class_decl).
    ctx.push_private_scope(super::build_private_scope(class, name));

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
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            let native_parent = match parent_name.as_str() {
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
                "Readable" | "Writable" | "Duplex" | "Transform"
                    if is_genuine_node_stream_parent(ctx, &parent_name) =>
                {
                    Some(("node_stream".to_string(), parent_name.clone()))
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
            let locally_shadowed = !ctx.class_renames.contains_key(&parent_name)
                && ctx.locals.lookup(&parent_name).is_some();
            if native_parent.is_some() && !locally_shadowed {
                (None, Some(parent_name), native_parent, None)
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
                // Do NOT set a static `extends` (parent_cid) here: the only
                // candidate would be `lookup_class(parent_name)`, which is the
                // wrong same-named module-global class we are deliberately
                // avoiding (wiring it would mis-route inherited-method / vtable
                // dispatch to that class's members). The dynamic `extends_expr`
                // path registers the correct parent edge at runtime via
                // `RegisterClassParentDynamic` + `function_class_id`.
                match lower_expr(ctx, super_class) {
                    Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                    Err(_) => (None, Some(parent_name), None, None),
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
                    match lower_expr(ctx, super_class) {
                        Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (None, Some(parent_name), None, None),
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
                match lower_expr(ctx, super_class) {
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
                match lower_expr(ctx, super_class) {
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
            match lower_expr(ctx, super_class) {
                Ok(expr) => (None, None, None, Some(Box::new(expr))),
                Err(_) => (None, None, None, None),
            }
        }
    } else {
        (None, None, None, None)
    };

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
    let mut static_accessor_names: Vec<String> = Vec::new();
    let mut static_accessor_fn_ids: Vec<FuncId> = Vec::new();
    let mut computed_members = Vec::new();
    let mut seen_generic_computed_member = false;

    for member in &class.body {
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
                    computed_members
                        .push(lower_generic_computed_class_member(ctx, method, computed)?);
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
                    _ => continue,
                };
                match method.kind {
                    ast::MethodKind::Getter => {
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_getter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let func = with_static_member_context(ctx, method.is_static, |ctx| {
                            lower_setter_method(ctx, method)
                        })?;
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
                            )?);
                        }
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        setters.push((prop_name, func));
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
                            methods.push(wrapper);
                            continue;
                        }
                        if seen_generic_computed_member && can_source_order_register {
                            computed_members.push(lower_noncomputed_class_member_registration(
                                ctx, method, &prop_name,
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
                if prop.declare {
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
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_setter(ctx, method)?;
                        if method.is_static {
                            static_accessor_names.push(prop_name.clone());
                            static_accessor_fn_ids.push(func.id);
                        }
                        setters.push((prop_name, func));
                    }
                }
            }
            ast::ClassMember::StaticBlock(block) => {
                let scope_mark = ctx.enter_scope();
                let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
                ctx.in_nonarrow_fn = true;
                let body = lower_block_stmt(ctx, &block.body)?;
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
        extends_name.as_deref(),
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
    })
}
