//! The `ast::Expr::Unary` arm of `lower_expr_impl`, extracted to a helper.
//! Pure code move — no behavior change.

use super::*;
use crate::lower::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

pub(crate) fn lower_unary_expr(ctx: &mut LoweringContext, unary: &ast::UnaryExpr) -> Result<Expr> {
    // AST-level typeof fold for `typeof Object.<known>` /
    // `typeof Array.<known>`. Lowering the operand would yield a
    // generic property-get on the global Object/Array (which
    // currently returns 0/undefined and makes `=== "function"`
    // checks fail). The static methods are real functions in
    // Node, so fold to the literal "function" string here.
    if matches!(unary.op, ast::UnaryOp::TypeOf) {
        // `typeof(x)` parenthesizes the operand, so the AST-level folds
        // below — which match a bare `Ident` / `Member` — would miss it
        // and fall through to a normal operand lowering. For an
        // unresolved identifier that means `typeof(zzz)` emitted a
        // ReferenceError-throwing get instead of folding to "undefined"
        // (the spec's GetValue-skips-on-typeof rule). Peel transparent
        // `Paren` wrappers so the operand-shape folds see through them.
        let typeof_arg = {
            let mut e = unary.arg.as_ref();
            while let ast::Expr::Paren(p) = e {
                e = p.expr.as_ref();
            }
            e
        };
        // #677: bare `typeof Function` — Function is a JS built-in
        // constructor, so typeof is "function". Without this fold,
        // the bare ident lowers to `GlobalGet(0)` and typeof reads
        // "object" via the global-this short-circuit.
        if let ast::Expr::Ident(id) = typeof_arg {
            if id.sym.as_ref() == "Function" && ctx.lookup_local("Function").is_none() {
                return Ok(Expr::String("function".to_string()));
            }
            // #2874: global `Iterator` (TC39 iterator-helpers) is a
            // constructor function in Node 22+.
            if id.sym.as_ref() == "Iterator"
                && ctx.lookup_local("Iterator").is_none()
                && ctx.lookup_func("Iterator").is_none()
            {
                return Ok(Expr::String("function".to_string()));
            }
            // #1454: global timer builtins and fetch are functions.
            // Timers still lower bare reads to ExternFuncRef; fetch
            // now resolves through globalThis for value identity.
            // Fold both shapes to "function".
            //
            // `gc` is included here. Unlike Node — where `gc` exists only
            // under `--expose-gc`, so `typeof gc === "undefined"` otherwise —
            // Perry's `gc()` is ALWAYS a real, callable builtin (a
            // call-intrinsic routed to `js_gc_collect`). Previously `typeof
            // gc` folded to a non-"function" value while `gc()` still worked,
            // so the idiomatic capability guard
            // `if (typeof gc === "function") gc()` (written precisely because
            // Node hides `gc` without `--expose-gc`) was always false. An
            // allocation-heavy program that gates collection on that guard
            // then never collected and RSS grew unbounded. Since Perry's `gc`
            // is genuinely available, `typeof gc` must be "function" so the
            // guard runs the collector.
            let n = id.sym.as_ref();
            if matches!(
                n,
                "setTimeout"
                    | "setInterval"
                    | "setImmediate"
                    | "clearTimeout"
                    | "clearInterval"
                    | "clearImmediate"
                    | "fetch"
                    // Callable global helpers that otherwise resolve to
                    // `GlobalGet(0)` (globalThis) for a bare read, so a
                    // value `typeof` reported "object" despite being
                    // fully callable. (#3986)
                    | "queueMicrotask"
                    | "structuredClone"
                    | "btoa"
                    | "atob"
                    | "gc"
            ) && ctx.lookup_local(n).is_none()
            {
                return Ok(Expr::String("function".to_string()));
            }
            // #1535: `import Stream from "node:stream"` should make
            // `typeof Stream === "function"` (legacy Stream
            // constructor with class statics hung off it). Perry
            // resolves the default import to a native-module
            // namespace today, so the read defaulted to typeof
            // "object". Fold when the local ident is bound as the
            // default import of a node module whose default export
            // Node exposes as a constructor function. (Other
            // modules whose default is a non-callable namespace —
            // `node:os`, `node:path` — stay typeof "object".)
            // Only the DEFAULT import (`import Stream from …`) folds to
            // "function". A namespace import (`import * as nsStream …`)
            // also registers as a native module with method `None`, but
            // it is a module namespace object — `typeof nsStream` must
            // stay "object" (#1535). Namespace imports additionally
            // register a builtin-module alias; default imports do not,
            // so the alias absence is the discriminator.
            if ctx.lookup_local(n).is_none() && ctx.lookup_builtin_module_alias(n).is_none() {
                if let Some((module_name, None)) = ctx.lookup_native_module(n) {
                    if matches!(module_name, "stream" | "node:stream") {
                        return Ok(Expr::String("function".to_string()));
                    }
                }
            }
            // #5373: in compiled external / compilePackages modules a
            // bare `require` is bound to a createRequire-backed closure
            // (see the ident-read arm), so `typeof require` is
            // "function" — matching Node CJS and enabling the common
            // `typeof require === 'function'` capability guard. Without
            // this, the generic non-throwing fold below reports
            // "undefined". Gated to external modules to mirror the
            // ident binding exactly.
            if n == "require" && ctx.is_external_module && ctx.lookup_local(n).is_none() {
                return Ok(Expr::String("function".to_string()));
            }
            if ctx.lookup_local(n).is_none()
                && ctx.lookup_func(n).is_none()
                && ctx.lookup_native_module(n).is_none()
                && ctx.lookup_imported_func(n).is_none()
                && ctx.lookup_class(n).is_none()
                && !is_builtin_function(n)
                && !is_known_global_identifier_name(n)
                && !matches!(n, "undefined" | "null" | "NaN" | "Infinity")
            {
                // #6062: a forward-referenced lexical (`let`/`const`/`class`
                // declared LATER in this or an enclosing same-function block) is
                // not yet a live local, so it reaches this "unresolvable" arm —
                // but per ECMA-262 `typeof` only skips GetValue for genuinely
                // UNRESOLVABLE references (undeclared globals). A declared-but-
                // uninitialized binding in its TDZ must throw ReferenceError,
                // exactly like a plain read. Route it to the throwing get. (A
                // `typeof` AFTER the declarator resolves via `lookup_local` and
                // never reaches here.)
                if ctx.forward_lexical_names.contains(n) {
                    return Ok(Expr::TypeOf(Box::new(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_global_get_or_throw_unresolved".to_string(),
                            param_types: vec![Type::Any],
                            return_type: Type::Any,
                        }),
                        args: vec![Expr::String(n.to_string())],
                        type_args: Vec::new(),
                        byte_offset: id.span.lo.0,
                    })));
                }
                // Not foldable to a compile-time "undefined": sloppy
                // implicit globals are runtime globalThis properties
                // (#3575), so `g = 5; typeof g` must observe the live
                // binding. Non-throwing lookup per the spec's
                // GetValue-skips-on-typeof rule.
                return Ok(Expr::TypeOf(Box::new(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_global_get_optional".to_string(),
                        param_types: vec![Type::Any],
                        return_type: Type::Any,
                    }),
                    args: vec![Expr::String(n.to_string())],
                    type_args: Vec::new(),
                    byte_offset: 0,
                })));
            }
        }
        // #1395: `typeof process.memoryUsage.rss` is a nested member
        // (`(process.memoryUsage).rss`) so it bypasses the
        // ident-receiver fold below. Node exposes `rss` as a fast-path
        // function hung off `process.memoryUsage`; fold to "function".
        if let ast::Expr::Member(outer) = typeof_arg {
            if let ast::MemberProp::Ident(outer_prop) = &outer.prop {
                if outer_prop.sym.as_ref() == "rss" {
                    if let ast::Expr::Member(inner) = outer.obj.as_ref() {
                        if let (ast::Expr::Ident(root), ast::MemberProp::Ident(mid)) =
                            (inner.obj.as_ref(), &inner.prop)
                        {
                            if root.sym.as_ref() == "process"
                                && mid.sym.as_ref() == "memoryUsage"
                                && ctx.lookup_local("process").is_none()
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                        }
                    }
                }
            }
        }
        if let ast::Expr::Member(member) = typeof_arg {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let obj_name = obj_ident.sym.as_ref();
                    let prop_name = prop_ident.sym.as_ref();
                    if matches!(prop_name, "encode" | "encodeInto")
                        && ctx
                            .lookup_local_type(obj_name)
                            .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                            .unwrap_or(false)
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    if prop_name == "decode"
                        && ctx
                            .lookup_local_type(obj_name)
                            .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                            .unwrap_or(false)
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #2143: `typeof Promise.resolve`, `typeof Math.min`,
                    // `typeof JSON.parse`, etc. — namespace static methods
                    // that Perry implements as codegen direct-call
                    // intrinsics. A bare value-read of these lowers to a
                    // numeric fallback (typeof "number"), but Node treats
                    // them as real functions. Folding to "function" here
                    // unblocks feature-detection idioms and the
                    // `.bind`/`.call`/`.apply` chain fold below. The
                    // existing Object/Array static method lists are
                    // subsumed by `is_known_namespace_static_function`.
                    if ctx.lookup_local(obj_name).is_none()
                        && ctx.lookup_func(obj_name).is_none()
                        && is_known_namespace_static_function(obj_name, prop_name)
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    let is_process_object = ctx.lookup_local(obj_name).is_none()
                        && (obj_name == "process"
                            || matches!(
                                ctx.lookup_builtin_module_alias(obj_name),
                                Some("process" | "node:process")
                            )
                            || matches!(
                                ctx.lookup_native_module(obj_name),
                                Some((
                                    "process"
                                        | "node:process"
                                        | "process.namespace"
                                        | "node:process.namespace"
                                        | "process.default"
                                        | "node:process.default",
                                    None
                                ))
                            ));
                    if is_process_object && prop_name == "sourceMapsEnabled" {
                        return Ok(Expr::String("boolean".to_string()));
                    }
                    // #1410 / #1400 / #1398 / #1409: `typeof
                    // process.ref` / `typeof process.unref` /
                    // `typeof process.setSourceMapsEnabled` /
                    // `typeof process.getBuiltinModule` /
                    // `typeof process.dlopen`. These methods
                    // lower to `Expr::Undefined` / no-ops when
                    // called; a bare member read still falls
                    // through to the generic process member path
                    // (returns 0 / "number" typeof), so fold to
                    // "function" here to match Node.
                    if is_process_object
                        && matches!(
                            prop_name,
                            "ref"
                                | "unref"
                                | "setSourceMapsEnabled"
                                | "getBuiltinModule"
                                | "dlopen"
                                | "hasUncaughtExceptionCaptureCallback"
                                | "setUncaughtExceptionCaptureCallback"
                                | "loadEnvFile"
                        )
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("async_hooks", "AsyncHook"))
                    ) && matches!(prop_name, "enable" | "disable")
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("async_hooks", "AsyncResource"))
                    ) && matches!(
                        prop_name,
                        "asyncId" | "triggerAsyncId" | "runInAsyncScope" | "emitDestroy" | "bind"
                    ) {
                        return Ok(Expr::String("function".to_string()));
                    }
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("events", "EventEmitterAsyncResource"))
                    ) && matches!(
                        prop_name,
                        "emitDestroy"
                            | "on"
                            | "addListener"
                            | "once"
                            | "prependListener"
                            | "prependOnceListener"
                            | "off"
                            | "removeListener"
                            | "removeAllListeners"
                            | "emit"
                            | "listenerCount"
                            | "listeners"
                            | "rawListeners"
                            | "eventNames"
                            | "setMaxListeners"
                            | "getMaxListeners"
                    ) {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #1320: `typeof obs.observe` on a PerformanceObserver
                    // instance. A bare member read on a native-class
                    // instance lowers to a 0-arg NativeMethodCall (getter
                    // semantics), so `typeof` evaluated `observe()` and
                    // reported "undefined". These are methods, not
                    // getters — fold to "function" (the call form
                    // `obs.observe(...)` is unaffected).
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("perf_hooks", _))
                    ) && matches!(prop_name, "observe" | "disconnect" | "takeRecords")
                    {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // `readline.Interface` is a native handle whose
                    // value-read members lower as zero-arg native
                    // calls. For shape probes, fold `typeof` at the
                    // AST layer so we report Node's public surface
                    // without invoking those methods.
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("readline", "Interface"))
                    ) {
                        if matches!(
                            prop_name,
                            "close"
                                | "pause"
                                | "resume"
                                | "prompt"
                                | "setPrompt"
                                | "getPrompt"
                                | "question"
                                | "write"
                                | "getCursorPos"
                                | "on"
                        ) {
                            return Ok(Expr::String("function".to_string()));
                        }
                        if prop_name == "line" {
                            return Ok(Expr::String("string".to_string()));
                        }
                        if prop_name == "terminal" {
                            return Ok(Expr::String("boolean".to_string()));
                        }
                    }
                    // #1698: `typeof req.json` on a Web Fetch Request /
                    // Response instance. The body methods are real
                    // functions in Node, but a bare LITERAL member read
                    // (`req.json`) takes the typed Web-Fetch codegen path,
                    // which returns the numeric handle (typeof "object")
                    // rather than routing to `dispatch_request_property`'s
                    // bound-method value (the COMPUTED `req[key]` form
                    // already does). Fold the literal-read typeof to
                    // "function" to match Node. The call form
                    // (`req.json()`) is unaffected.
                    if matches!(
                        ctx.lookup_native_instance(obj_name),
                        Some(("Request", "Request")) | Some(("fetch", "Response"))
                    ) && matches!(
                        prop_name,
                        "json" | "text" | "arrayBuffer" | "blob" | "bytes" | "formData" | "clone"
                    ) {
                        return Ok(Expr::String("function".to_string()));
                    }
                    // #677: `typeof Function.prototype` → "object".
                    // `Function.prototype` is the (immutable) prototype
                    // chain root for all functions; in Node typeof is
                    // "object". Other `Function.<X>` reads (`Function.name`,
                    // etc.) fall through to GlobalGet member-access,
                    // which today returns `undefined`.
                    if obj_name == "Function"
                        && prop_name == "prototype"
                        && ctx.lookup_local("Function").is_none()
                    {
                        return Ok(Expr::String("object".to_string()));
                    }
                }
            }
            // `typeof "".methodName === "function"` — feature
            // detection idiom. Generic PropertyGet on a string
            // literal returns undefined in Perry today, so the
            // typeof would be "undefined" and the test branch
            // gets skipped. Fold to "function" when the property
            // name is a known String.prototype method that the
            // runtime actually dispatches.
            if let (ast::Expr::Lit(ast::Lit::Str(_)), ast::MemberProp::Ident(prop_ident)) =
                (member.obj.as_ref(), &member.prop)
            {
                let prop_name = prop_ident.sym.as_ref();
                if is_known_string_prototype_method(prop_name) {
                    return Ok(Expr::String("function".to_string()));
                }
            }
            // #1777: `typeof Array.prototype.slice` / `typeof [].slice`
            // (and String/Number/Boolean prototypes). The method value
            // read lowers to `undefined` today, so typeof was
            // "undefined" — but these are real functions in Node and the
            // `.call`/`.apply` dispatch is now wired (see
            // `try_builtin_prototype_method_apply_call`). Fold to
            // "function" for known prototype methods so feature
            // detection (`typeof X.slice === "function"`) agrees.
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                // `<Ctor>.prototype.<method>`
                if let ast::Expr::Member(proto) = member.obj.as_ref() {
                    if let (ast::Expr::Ident(base), ast::MemberProp::Ident(proto_prop)) =
                        (proto.obj.as_ref(), &proto.prop)
                    {
                        let ctor = base.sym.as_ref();
                        if proto_prop.sym.as_ref() == "prototype"
                            && ctx.lookup_local(ctor).is_none()
                        {
                            // #2058: every built-in prototype inherits the
                            // universal `Object.prototype` methods
                            // (`isPrototypeOf`, `hasOwnProperty`,
                            // `toString`, …), so `typeof
                            // Object.prototype.isPrototypeOf` /
                            // `typeof Number.prototype.hasOwnProperty` are
                            // "function" in Node. Plus each ctor's own
                            // prototype methods (and `Function.prototype`'s
                            // `call`/`apply`/`bind`).
                            let is_obj_proto = is_known_object_prototype_method(prop_name);
                            let is_fn = match ctor {
                                "Object" => is_obj_proto,
                                "Function" => {
                                    is_obj_proto || matches!(prop_name, "call" | "apply" | "bind")
                                }
                                "Array" => {
                                    is_obj_proto || is_known_array_prototype_method(prop_name)
                                }
                                "String" => {
                                    is_obj_proto || is_known_string_prototype_method(prop_name)
                                }
                                // Number/Boolean prototypes: the handful of
                                // ctor-specific methods plus the inherited
                                // Object.prototype methods are all functions.
                                "Number" => {
                                    is_obj_proto
                                        || matches!(
                                            prop_name,
                                            "toFixed" | "toPrecision" | "toExponential"
                                        )
                                }
                                "Boolean" => is_obj_proto,
                                "TextEncoder" => {
                                    is_obj_proto || matches!(prop_name, "encode" | "encodeInto")
                                }
                                "TextDecoder" => is_obj_proto || prop_name == "decode",
                                _ => false,
                            };
                            if is_fn {
                                return Ok(Expr::String("function".to_string()));
                            }
                        }
                    }
                }
                // `[].<method>` — array-literal prototype borrow.
                if matches!(member.obj.as_ref(), ast::Expr::Array(_))
                    && is_known_array_prototype_method(prop_name)
                {
                    return Ok(Expr::String("function".to_string()));
                }
                // #2143: `typeof Promise.resolve.bind` /
                // `typeof Math.min.call` / `typeof JSON.parse.apply`.
                // Built-in function values don't inherit
                // `Function.prototype` in Perry's representation, so the
                // chained `.bind`/`.call`/`.apply` read falls through to
                // a numeric fallback (typeof "number"). Node treats
                // these as real functions — fold here when the inner
                // member names a known namespace static so feature
                // detection (Test262 `propertyHelper.js`, the Promise
                // tests cited in #793) sees callable values.
                if matches!(prop_name, "bind" | "call" | "apply") {
                    if let ast::Expr::Member(inner) = member.obj.as_ref() {
                        if let (ast::Expr::Ident(inner_obj), ast::MemberProp::Ident(inner_prop)) =
                            (inner.obj.as_ref(), &inner.prop)
                        {
                            let inner_obj_name = inner_obj.sym.as_ref();
                            let inner_prop_name = inner_prop.sym.as_ref();
                            if ctx.lookup_local(inner_obj_name).is_none()
                                && ctx.lookup_func(inner_obj_name).is_none()
                                && is_known_namespace_static_function(
                                    inner_obj_name,
                                    inner_prop_name,
                                )
                            {
                                return Ok(Expr::String("function".to_string()));
                            }
                        }
                    }
                }
            }
        }
    }
    // Static `delete` folding only applies when no `with` environment
    // is active: inside `with(o) { delete x }`, `x` may resolve to a
    // configurable property of `o` and must be deleted at runtime
    // (Test262 11.4.1-4.a-6), so we leave those to the dynamic path.
    if unary.op == ast::UnaryOp::Delete && ctx.with_env_stack.is_empty() {
        // Peel parens: `delete (x)` deletes the inner reference.
        let mut bare = unary.arg.as_ref();
        while let ast::Expr::Paren(p) = bare {
            bare = p.expr.as_ref();
        }
        if let ast::Expr::Member(member) = bare {
            if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                (member.obj.as_ref(), &member.prop)
            {
                let obj_name = obj.sym.as_ref();
                let prop_name = prop.sym.as_ref();
                let is_global =
                    ctx.lookup_local(obj_name).is_none() && ctx.lookup_func(obj_name).is_none();
                if is_global
                    && obj_name == "Number"
                    && matches!(
                        prop_name,
                        "NaN"
                            | "POSITIVE_INFINITY"
                            | "NEGATIVE_INFINITY"
                            | "MAX_VALUE"
                            | "MIN_VALUE"
                            | "EPSILON"
                            | "MAX_SAFE_INTEGER"
                            | "MIN_SAFE_INTEGER"
                    )
                {
                    return Ok(Expr::Bool(false));
                }
                // `Math`'s numeric constants are non-configurable, so
                // `delete Math.PI` is `false` (Math's *methods* stay
                // configurable, hence `delete Math.abs` is `true` and
                // is left to the generic path). Test262 S8.12.7_A1.
                if is_global
                    && obj_name == "Math"
                    && matches!(
                        prop_name,
                        "E" | "LN10" | "LN2" | "LOG10E" | "LOG2E" | "PI" | "SQRT1_2" | "SQRT2"
                    )
                {
                    return Ok(Expr::Bool(false));
                }
            }
        }
        // `delete <BindingIdentifier>` — deleting a reference to a
        // resolvable binding (var / let / const / function / param /
        // class / import) is non-configurable, so it evaluates to
        // `false` without removing anything (spec 13.5.1.2). The bare
        // globals `undefined` / `NaN` / `Infinity` are likewise
        // non-configurable global properties → `false`. Any other
        // unresolvable bare identifier (an implicit global from
        // `x = 1`, or a configurable global builtin) is `true` in
        // sloppy mode — lowering it as a literal avoids the spurious
        // ReferenceError the operand-evaluation path would throw.
        if let ast::Expr::Ident(id) = bare {
            let name = id.sym.as_ref();
            // Bare globals that are non-configurable → false.
            if name == "arguments" || matches!(name, "undefined" | "NaN" | "Infinity") {
                return Ok(Expr::Bool(false));
            }
            if let Some(lid) = ctx.lookup_local(name) {
                // `x = 1` with no declaration creates a *configurable*
                // global property (`delete x` → true); a real
                // var/let/const/param binding is non-configurable
                // (→ false). Distinguish via the implicit-global set.
                if ctx.sloppy_implicit_global_ids.contains(&lid) {
                    return Ok(Expr::Bool(true));
                }
                // Any resolvable binding NOT in the implicit-global set is a
                // real `var`/`let`/`const`/param declaration — non-configurable,
                // so `delete <ident>` is `false` (spec 13.5.1.2), at module top
                // level too. A module-level bare `x = 1` implicit global is
                // distinguishable: `define_sloppy_implicit_global` records it in
                // `sloppy_implicit_global_ids` even at scope depth 0, so it took
                // the `true` arm above (Test262 S11.4.1_A3.2_T1). Reaching here
                // therefore means a genuine `var x = 1` / `let` / `const`
                // binding, which must be `false` (Test262 S11.4.1_A3.1) rather
                // than deferred to a runtime delete that removes it and returns
                // `true`.
                return Ok(Expr::Bool(false));
            } else if ctx.lookup_func(name).is_some()
                || ctx.lookup_class(name).is_some()
                || ctx.lookup_imported_func(name).is_some()
            {
                return Ok(Expr::Bool(false));
            } else {
                // Truly unresolvable bare identifier (no binding, no
                // known global) → `true` in sloppy mode; lowering it as
                // a literal avoids a spurious ReferenceError from the
                // operand-evaluation path.
                return Ok(Expr::Bool(true));
            }
        }
    }
    let operand = Box::new(lower_expr(ctx, &unary.arg)?);
    match unary.op {
        ast::UnaryOp::Minus => {
            // Fold -Number into Number(-val) to simplify codegen
            // (e.g., array literals with negative numbers avoid Unary wrapper)
            if let Expr::Number(val) = *operand {
                Ok(Expr::Number(-val))
            } else if let Expr::Integer(val) = *operand {
                // Special case: -0 must be preserved as -0.0 (negative zero)
                // because integers collapse +0 and -0 into the same bit pattern.
                // JS distinguishes these in `console.log`, `Object.is`, and
                // `1/x` — so fold to Number(-0.0) instead of Integer(0).
                if val == 0 {
                    Ok(Expr::Number(-0.0))
                } else {
                    Ok(Expr::Integer(-val))
                }
            } else {
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    operand,
                })
            }
        }
        ast::UnaryOp::Plus => Ok(Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        }),
        ast::UnaryOp::Bang => Ok(Expr::Unary {
            op: UnaryOp::Not,
            operand,
        }),
        ast::UnaryOp::Tilde => Ok(Expr::Unary {
            op: UnaryOp::BitNot,
            operand,
        }),
        ast::UnaryOp::TypeOf => {
            // Fast path: known Symbol-producing expressions resolve to "symbol"
            // at compile time (avoids needing runtime js_value_typeof to
            // recognize the SymbolHeader magic).
            if matches!(&*operand, Expr::SymbolNew(_) | Expr::SymbolFor(_)) {
                return Ok(Expr::String("symbol".to_string()));
            }
            Ok(Expr::TypeOf(operand))
        }
        ast::UnaryOp::Delete => {
            // `delete super.prop` / `delete super[expr]` is always a
            // ReferenceError (the operand is a SuperProperty reference,
            // which `delete` rejects). Peel parens to catch
            // `delete (super.x)`. Args of a computed super key are
            // evaluated first for side effects.
            let mut del_arg = unary.arg.as_ref();
            while let ast::Expr::Paren(p) = del_arg {
                del_arg = p.expr.as_ref();
            }
            if let ast::Expr::SuperProp(super_prop) = del_arg {
                let throw = throw_reference_error_expr("js_throw_reference_error_super_delete");
                if let ast::SuperProp::Computed(computed) = &super_prop.prop {
                    let key = lower_expr(ctx, computed.expr.as_ref())?;
                    return Ok(Expr::Sequence(vec![key, throw]));
                }
                return Ok(throw);
            }
            // Proxy delete: rewrite `delete proxy.key` as ProxyDelete.
            if let Expr::ProxyGet { proxy, key } = &*operand {
                return Ok(Expr::ProxyDelete {
                    proxy: proxy.clone(),
                    key: key.clone(),
                });
            }
            Ok(Expr::Delete(operand))
        }
        ast::UnaryOp::Void => Ok(Expr::Void(operand)),
        // #853: `ast::UnaryOp` is `#[non_exhaustive]` upstream — keep
        // this catch-all as a forward-compat safety net.
        #[allow(unreachable_patterns)]
        _ => Err(anyhow!("Unsupported unary operator: {:?}", unary.op)),
    }
}
