//! Member-lowering tail: receiver lowering, builtin-static reroute-undo,
//! `.name`/`.length` folds, the #463 unimplemented-API gate, and the
//! final PropertyGet/IndexGet/private dispatch.
//!
//! Split out of `expr_member.rs` (pure code move). Runs after the
//! early-return checks in `lower_member_inner`.

use crate::types::Type;
use anyhow::Result;
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};

use super::*;

pub(crate) fn lower_member_tail(
    ctx: &mut LoweringContext,
    member: &ast::MemberExpr,
    member_is_call_callee: bool,
) -> Result<Expr> {
    let obj_span = member.obj.as_ref().span();
    let mut object_expr = match ctx.prelowered_member_receiver.take() {
        Some((key, lowered)) if key == (obj_span.lo.0, obj_span.hi.0) => lowered,
        _ => lower_expr(ctx, &member.obj)?,
    };
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if let Some(value) = ws_ready_state_value(prop_ident.sym.as_ref()) {
            if is_ws_ready_state_receiver(ctx, member.obj.as_ref(), &object_expr) {
                return Ok(Expr::Number(value));
            }
        }
        // #4533/#4561: `Error.isPrototypeOf(x)`, `Number.bind(...)`, etc. read an
        // inherited Function/Object prototype method off a builtin constructor.
        // Those builtin idents otherwise collapse to bare `GlobalGet(0)`
        // (globalThis) in the static-member path below, so the predicate ran
        // against globalThis instead of the real constructor. Resolve the
        // builtin to its globalThis property so the receiver is the constructor.
        if matches!(
            prop_ident.sym.as_ref(),
            "bind" | "call" | "apply" | "isPrototypeOf"
        ) {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.as_ref();
                if crate::analysis::is_builtin_global_value_name(obj_name) {
                    object_expr = Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::GlobalGet(0)),
                        property: obj_name.to_string(),
                    };
                }
            }
        }
    }
    let member_object_is_global_this = matches!(
        unwrap_transparent(member.obj.as_ref()),
        ast::Expr::Ident(i) if i.sym.as_ref() == "globalThis"
    ) || matches!(&object_expr, Expr::LocalGet(id) if ctx.global_this_aliases.contains(id));
    let member_reads_global_fetch = member_object_is_global_this
        && match &member.prop {
            ast::MemberProp::Ident(p) => matches!(
                p.sym.as_ref(),
                "fetch" | "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
            ),
            ast::MemberProp::Computed(c) => {
                matches!(
                    c.expr.as_ref(),
                    ast::Expr::Lit(ast::Lit::Str(s))
                        if matches!(
                            s.value.as_str(),
                            Some(
                                "fetch"
                                    | "Blob"
                                    | "File"
                                    | "FormData"
                                    | "Headers"
                                    | "Request"
                                    | "Response"
                            )
                        )
                )
            }
            ast::MemberProp::PrivateName(_) => false,
        };
    if member_reads_global_fetch {
        ctx.uses_fetch = true;
    }

    // #973 (5ddccbbc) rerouted bare built-in identifiers used as VALUES
    // (`Number`, `Object`, `Array`, ...) to `PropertyGet { GlobalGet(0),
    // name }` so identity comparisons like `inst.constructor === Date`
    // resolve both sides to the same `populate_global_this_builtins`
    // closure. But when the built-in ident is the OBJECT of a member
    // access (`Number.parseFloat`, `Object.keys`, `Array.isArray`, ...),
    // that reroute turns the intrinsic static-method/property lookup into
    // `globalThis.Number.parseFloat`, which is no longer the same value
    // as the intrinsic global `parseFloat` — silently breaking
    // `Number.parseFloat === parseFloat`, `Number.parseInt === parseInt`,
    // and similar identity checks (regressed test_gap_number_math).
    // Static surfaces must keep the pre-#973 intrinsic `GlobalGet(0)`
    // dispatch. Detect and undo the reroute only in member-object
    // position; local shadowing is unaffected because a shadowing local
    // would have lowered to `LocalGet`, never this reroute.
    if let Expr::PropertyGet {
        object: inner,
        property,
        ..
    } = &object_expr
    {
        if matches!(inner.as_ref(), Expr::GlobalGet(0))
            && (crate::analysis::is_builtin_global_value_name(property)
                // #4139: `Math`/`JSON`/`Reflect` bare values now lower to
                // `PropertyGet { GlobalGet(0), <name> }` (see lower_expr.rs) so
                // reflection sees the real namespace object. But in member-OBJECT
                // position (`Math.max(…)`, `JSON.stringify(…)`, `Reflect.get(…)`)
                // the intrinsic call / constant-fold paths expect the bare
                // `GlobalGet(0)` receiver — undo the reroute here exactly as for
                // the built-in constructors, keeping those paths byte-identical.
                || matches!(property.as_str(), "Math" | "JSON" | "Reflect"))
        {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if obj_ident.sym.as_ref() == property.as_str() && property != "globalThis" {
                    // #2060 / #2142 / #2145: `<Ctor>.prototype` and
                    // `<Ctor>.__proto__` must keep reading the constructor
                    // closure's real proto / static-prototype. Each built-in
                    // constructor closure carries a populated proto (allocated
                    // in `populate_global_this_builtins`, populated by
                    // `populate_builtin_prototype_methods`) — that is where
                    // typed-array accessor descriptors AND the reified
                    // built-in prototype method values live. For
                    // `__proto__`, typed-array constructors are linked to the
                    // shared `%TypedArray%` intrinsic via
                    // `closure_set_static_prototype` (#2145); collapsing here
                    // would drop the receiver, and codegen lowers
                    // `globalThis.__proto__` through the no-name path → literal
                    // `0.0` (a number), which is the symptom reported in #2145.
                    // Accept BOTH `Ctor.prototype` and `Ctor[\"prototype\"]`
                    // (string-literal computed key) — the reified constructor
                    // receiver must survive for either form, else the read
                    // collapses to `globalThis.prototype` (undefined). Test262
                    // property-accessors `Ctor[\"prototype\"]`.
                    let outer_is_prototype_or_proto = matches!(
                        static_member_prop_name(&member.prop).as_deref(),
                        Some("prototype") | Some("__proto__")
                    );
                    let receiver_is_namespace_value = matches!(
                        property.as_str(),
                        "Atomics"
                            | "crypto"
                            | "WebAssembly"
                            | "Temporal"
                            | "localStorage"
                            | "sessionStorage"
                    );
                    let outer_is_websocket_static = property == "WebSocket"
                        && match &member.prop {
                            ast::MemberProp::Ident(p) => matches!(
                                p.sym.as_ref(),
                                "CONNECTING" | "OPEN" | "CLOSING" | "CLOSED"
                            ),
                            ast::MemberProp::Computed(_) => true,
                            _ => false,
                        };
                    let outer_is_reified_object_static_value = property == "Object"
                        && matches!(
                            &member.prop,
                            ast::MemberProp::Ident(p) if matches!(
                                p.sym.as_ref(),
                                // Keep in sync with the Object statics installed
                                // on the reified constructor in
                                // global_this/install_static.rs — every name
                                // here resolves to a real native function object
                                // (correct `typeof`/`.name`/`.length`) when read
                                // as a value (`const f = Object.isExtensible`).
                                // Omitting one collapses the read to the
                                // intrinsic `GlobalGet(0).<name>` path, which has
                                // no value form and yields `undefined` (#5588).
                                "assign"
                                    | "create"
                                    | "defineProperties"
                                    | "defineProperty"
                                    | "entries"
                                    | "freeze"
                                    | "fromEntries"
                                    | "getOwnPropertyDescriptor"
                                    | "getOwnPropertyDescriptors"
                                    | "getOwnPropertyNames"
                                    | "getOwnPropertySymbols"
                                    | "getPrototypeOf"
                                    | "groupBy"
                                    | "hasOwn"
                                    | "is"
                                    | "isExtensible"
                                    | "isFrozen"
                                    | "isSealed"
                                    | "keys"
                                    | "preventExtensions"
                                    | "seal"
                                    | "setPrototypeOf"
                                    | "values"
                            )
                        );
                    // #4437: value reads such as `JSON.stringify` /
                    // `Reflect.apply` / `BigInt.asIntN` / `Symbol.for` /
                    // `Promise.resolve` need the reified namespace/constructor
                    // receiver. Direct calls still take the intrinsic path this
                    // reroute-undo protects.
                    let outer_static_member = match &member.prop {
                        ast::MemberProp::Ident(p) => Some(p.sym.as_ref()),
                        ast::MemberProp::Computed(c) => match c.expr.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                            _ => None,
                        },
                        ast::MemberProp::PrivateName(_) => None,
                    };
                    // #4596 follow-up: `Array.isArray` / `Array.from` /
                    // `Array.of` read as VALUES need the reified Array
                    // constructor receiver so they resolve to the real native
                    // function objects (correct `.name` / `.length`). They are
                    // installed with metadata via `install_constructor_static`
                    // (global_this.rs), but the reroute-undo otherwise collapses
                    // them to `GlobalGet(0).<name>`, whose intrinsic path drops
                    // the metadata (`typeof` is "function" but `.name` was
                    // undefined). `Array.fromAsync` is unreified and stays
                    // undefined either way. Direct calls keep the intrinsic
                    // fast path via the `!member_is_call_callee` gate.
                    // #4627: all six Number statics (isFinite / isInteger /
                    // isNaN / isSafeInteger / parseFloat / parseInt) are reified
                    // with metadata via install_constructor_static, so routing
                    // value reads to the reified Number receiver is safe and
                    // fixes the missing `.name`/`.length` on isInteger /
                    // isSafeInteger. (String's fromCharCode/etc. are NOT reified
                    // yet — left to #4627.)
                    let outer_is_reified_builtin_static_value = !member_is_call_callee
                        && matches!(
                            property.as_str(),
                            "JSON"
                                | "Reflect"
                                | "BigInt"
                                | "Symbol"
                                | "Array"
                                | "Number"
                                | "Promise"
                        )
                        && outer_static_member
                            .map(|member| {
                                crate::analysis::is_builtin_static_function_member(property, member)
                            })
                            .unwrap_or(false);
                    // Non-callee `console.log` reads need the namespace
                    // receiver; the property-only GlobalGet path collides
                    // with detached `Math.log`.
                    let receiver_is_detached_console_read =
                        property == "console" && !member_is_call_callee;
                    // #4596: `Date.now` / `Date.parse` / `Date.UTC` read as a
                    // VALUE needs the reified Date constructor receiver so it
                    // resolves to the real native function object (typeof
                    // "function", correct `.name`/`.length`, callable). Undoing
                    // the reroute collapses it to `GlobalGet(0).now`, for which
                    // codegen has no intrinsic handler (unlike `Object.keys` /
                    // `Math.max`) — so the read mis-folds to a number. Direct
                    // CALLS (`Date.now()`) are intercepted earlier as
                    // `Expr::DateNow` / `DateParse` / `DateUtc`, so gate on a
                    // non-callee read.
                    let outer_is_reified_date_static_value = !member_is_call_callee
                        && property == "Date"
                        && outer_static_member
                            .map(|member| matches!(member, "now" | "parse" | "UTC"))
                            .unwrap_or(false);
                    // #4627: `String.fromCharCode` / `fromCodePoint` / `raw` are
                    // reified statics — value reads need the reified String
                    // receiver for correct `.name`/`.length`. Explicit member
                    // list (NOT the whole namespace) so only the reified statics
                    // are rerouted.
                    let outer_is_reified_string_static_value = !member_is_call_callee
                        && property == "String"
                        && outer_static_member
                            .map(|member| {
                                matches!(member, "fromCharCode" | "fromCodePoint" | "raw")
                            })
                            .unwrap_or(false);
                    // #4521: `Promise.resolve` / `reject` / `all` / `race` /
                    // `allSettled` / `any` / `withResolvers` / `try` read as
                    // VALUES need the reified Promise constructor receiver so
                    // they resolve to the real native function objects (correct
                    // `.name` / `.length`, callable via reference / `.call`).
                    // They are installed with metadata via
                    // `install_constructor_static` (global_this.rs); the
                    // reroute-undo otherwise collapses them to
                    // `GlobalGet(0).<name>` (undefined). Direct calls
                    // (`Promise.all([...])`) take the codegen fast path via the
                    // `!member_is_call_callee` gate.
                    let outer_is_reified_promise_static_value = !member_is_call_callee
                        && property == "Promise"
                        && outer_static_member
                            .map(|member| {
                                matches!(
                                    member,
                                    "resolve"
                                        | "reject"
                                        | "all"
                                        | "race"
                                        | "allSettled"
                                        | "any"
                                        | "withResolvers"
                                        | "try"
                                )
                            })
                            .unwrap_or(false);
                    // #4533/#4561: inherited Object/Function prototype methods
                    // (`Error.isPrototypeOf`, `Number.valueOf`, `Object.bind`)
                    // must keep the real constructor receiver, not collapse to
                    // bare `GlobalGet(0)` — otherwise the predicate/dispatch runs
                    // against globalThis. The reroute above already resolved the
                    // receiver to `globalThis.<ctor>`; don't undo it here.
                    // #5135: `toString` is a universal inherited method too —
                    // `Function.toString` / `Array.toString` resolve to a real
                    // function in Node. Without keeping the reified constructor
                    // receiver the read collapses to `globalThis.toString`,
                    // which codegen folds to a number, so
                    // `Function.toString.call(Ctor)` (immer's `isPlainObject`)
                    // threw "call on a non-function".
                    let outer_is_inherited_object_proto_method = matches!(
                        outer_static_member,
                        Some(
                            "hasOwnProperty"
                                | "isPrototypeOf"
                                | "propertyIsEnumerable"
                                | "toLocaleString"
                                | "toString"
                                | "valueOf"
                        )
                    );
                    let outer_is_inherited_function_proto_method =
                        matches!(outer_static_member, Some("bind" | "call" | "apply"));
                    // #5897: `RegExp` has NO intrinsic static-member fast path in
                    // Perry — nothing downstream keys on the collapsed
                    // `GlobalGet(0).<prop>` shape for it (no reified statics are
                    // installed for `RegExp`, and its `.prototype` / `.length` /
                    // `.name` reads are folded by the dedicated arms above and
                    // below, off the AST receiver rather than `object_expr`).
                    // Collapsing can therefore only LOSE the receiver: an
                    // unrecognized static read resolved against `globalThis`
                    // instead of the constructor, so after
                    // `Function.prototype.indicator = 1` the spec-mandated
                    // `RegExp.indicator` (inherited from `Function.prototype`,
                    // which is `RegExp`'s [[Prototype]]) came back `undefined`
                    // rather than `1` (test262 built-ins/RegExp/S15.10.5_A2_T2).
                    // Keeping the receiver lets the runtime walk the constructor's
                    // prototype chain, which already resolves inherited props.
                    //
                    // The same receiver-drop affects the OTHER built-in
                    // constructors (`Array.indicator`, `Object.indicator`, …), but
                    // there the collapsed shape IS load-bearing — the intrinsic
                    // call/constant-fold paths for `Array.isArray`, `Object.keys`,
                    // `Math.PI`, … depend on it — so lifting it for those needs a
                    // complete per-builtin intrinsic-member table and is tracked
                    // separately.
                    //
                    // #5908: the `Function` constructor is the same safe case as
                    // `RegExp`. It has no intrinsic static-member fast path keyed
                    // on the collapsed shape (`Function.length` folds off either
                    // the bare `GlobalGet(0)` or the `PropertyGet { GlobalGet(0),
                    // "Function" }` value-form in the `.length` arm below, and
                    // `Function.name` / `Function.prototype` / `Function.{call,
                    // apply,bind}` are handled by their own dedicated arms), so
                    // collapsing only LOSES the receiver — after
                    // `Function.prototype.indicator = 1`, reading `Function.indicator`
                    // (inherited via the ctor's [[Prototype]] = `%Function.prototype%`)
                    // came back `undefined` instead of `1` (test262
                    // built-ins/Function/S15.3.3_A2_T2). Keeping the receiver lets the
                    // runtime walk the prototype chain, which already resolves the
                    // inherited property (`closure_get_dynamic_prop`'s
                    // `function_prototype_fallback_target`).
                    let receiver_is_regexp_ctor = property == "RegExp";
                    let receiver_is_function_ctor = property == "Function";
                    if !outer_is_prototype_or_proto
                        && !receiver_is_namespace_value
                        && !receiver_is_regexp_ctor
                        && !receiver_is_function_ctor
                        && !outer_is_websocket_static
                        && !outer_is_reified_object_static_value
                        && !outer_is_reified_builtin_static_value
                        && !outer_is_reified_date_static_value
                        && !outer_is_reified_string_static_value
                        && !outer_is_reified_promise_static_value
                        && !outer_is_inherited_object_proto_method
                        && !outer_is_inherited_function_proto_method
                        && !receiver_is_detached_console_read
                    {
                        object_expr = Expr::GlobalGet(0);
                    }
                }
            }
        }
    }

    // #2144: spec `.name` own-property on built-in functions / constructors.
    //
    // Built-in constructors (`TypeError`, `Promise`, `Array`, …) and the
    // static functions on built-in namespaces / constructors (`Math.min`,
    // `Promise.race`, `Array.isArray`, …) are not represented as named
    // closure values in Perry. Reading their `.name` therefore falls through
    // to a globalThis lookup that returns 0/undefined instead of the spec
    // name string. `assert.throws` reports `expectedErrorConstructor.name`
    // and Test262 regularly inspects built-in `.name`, so fold these reads
    // here at lowering time when the receiver shape is unambiguous.
    //
    // Detection is gated on the *lowered* receiver expression — bare
    // `GlobalGet(0)` (after the reroute-undo above for `TypeError.name`) or
    // `PropertyGet { GlobalGet(0), <method> }` (for `Math.min.name` /
    // `Promise.race.name`). Local shadowing (`const Math = …`) lowers the
    // receiver to a `LocalGet` instead, so the fold is correctly skipped.
    // #3143: spec `.length` own-property on built-in constructors. Same
    // gating as the `.name` fold below — bare `GlobalGet(0)` receiver (no
    // local shadowing) and a recognized standard constructor name. Built-in
    // constructors share a no-op closure thunk with no per-name arity, so a
    // value-read would otherwise return 0 instead of the spec count
    // (`Array.length === 1`, `Date.length === 7`).
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if prop_ident.sym.as_ref() == "length" {
            // Peel transparent TS/paren wrappers so `(Array as any).length` —
            // the pervasive Test262 / cast idiom — folds the same as the bare
            // `Array.length`.
            let mut recv = member.obj.as_ref();
            loop {
                recv = match recv {
                    ast::Expr::TsAs(x) => x.expr.as_ref(),
                    ast::Expr::TsNonNull(x) => x.expr.as_ref(),
                    ast::Expr::TsSatisfies(x) => x.expr.as_ref(),
                    ast::Expr::TsTypeAssertion(x) => x.expr.as_ref(),
                    ast::Expr::TsConstAssertion(x) => x.expr.as_ref(),
                    ast::Expr::Paren(x) => x.expr.as_ref(),
                    _ => break,
                };
            }
            if let ast::Expr::Ident(obj_ident) = recv {
                let name = obj_ident.sym.as_ref();
                // The receiver must resolve to the *global* builtin (not a
                // local shadow). A bare ident lowers to `GlobalGet(0)` (after
                // the reroute-undo above); wrapped in a cast/paren it keeps the
                // #973 value-form `PropertyGet { GlobalGet(0), <name> }`. A
                // shadowing local would lower to `LocalGet`, matching neither —
                // so the fold is correctly skipped.
                let is_global_builtin = match &object_expr {
                    Expr::GlobalGet(0) => true,
                    Expr::PropertyGet {
                        object, property, ..
                    } => matches!(object.as_ref(), Expr::GlobalGet(0)) && property.as_str() == name,
                    _ => false,
                };
                if is_global_builtin {
                    if let Some(len) = crate::analysis::builtin_constructor_length(name)
                        .or_else(|| crate::analysis::builtin_global_function_length(name))
                    {
                        return Ok(Expr::Number(len as f64));
                    }
                }
            }
            if let Expr::PropertyGet {
                object: inner,
                property,
                ..
            } = &object_expr
            {
                if matches!(inner.as_ref(), Expr::GlobalGet(0)) {
                    if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                        if let (ast::Expr::Ident(ns_ident), ast::MemberProp::Ident(method_ident)) =
                            (inner_member.obj.as_ref(), &inner_member.prop)
                        {
                            let ns = ns_ident.sym.as_ref();
                            let method = method_ident.sym.as_ref();
                            if method == property.as_str() {
                                if let Some(len) =
                                    crate::analysis::builtin_static_function_length(ns, method)
                                {
                                    return Ok(Expr::Number(len as f64));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if prop_ident.sym.as_ref() == "name" {
            match &object_expr {
                Expr::GlobalGet(0) => {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        let name = obj_ident.sym.as_ref();
                        if crate::analysis::is_builtin_global_value_name(name) {
                            return Ok(Expr::String(name.to_string()));
                        }
                    }
                }
                Expr::PropertyGet {
                    object: inner,
                    property,
                    ..
                } => {
                    if matches!(inner.as_ref(), Expr::GlobalGet(0)) {
                        if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                            if let (
                                ast::Expr::Ident(ns_ident),
                                ast::MemberProp::Ident(method_ident),
                            ) = (inner_member.obj.as_ref(), &inner_member.prop)
                            {
                                let ns = ns_ident.sym.as_ref();
                                let method = method_ident.sym.as_ref();
                                if method == property.as_str()
                                    && crate::analysis::is_builtin_static_function_member(
                                        ns, method,
                                    )
                                {
                                    return Ok(Expr::String(method.to_string()));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let object = Box::new(object_expr);

    // Unimplemented-API gate (#463). When the receiver is a
    // `NativeModuleRef("crypto")`-style import binding and the user is
    // reading a named property, fail loudly if the manifest doesn't
    // know about that property. The check is gated on the module
    // having at least one entry in `API_MANIFEST`, so modules whose
    // surface hasn't been enumerated yet (incremental coverage) keep
    // working — adding entries to a module promotes it to strict mode
    // automatically.
    //
    // Stubs (`stub: true` in the manifest) are NOT treated as
    // unimplemented — those are intentional no-ops surfaced by #464's
    // runtime first-call warning. The call only checks that
    // `module_has_symbol` returns Some; the stub flag is consulted by
    // the docs serializer, not by the gate.
    //
    // Escape hatch: setting `PERRY_ALLOW_UNIMPLEMENTED=1` skips the
    // check entirely (downgrades to existing silent-undefined
    // behavior). Useful when the manifest has a real gap that a
    // followup will fix; documents the bypass instead of forcing an
    // unrelated change in this PR.
    if let (Expr::NativeModuleRef(module), ast::MemberProp::Ident(prop_ident)) =
        (&*object, &member.prop)
    {
        let prop = prop_ident.sym.as_ref();
        // Skip the gate when `member.obj` is an Ident that was a
        // *named* import binding from the module (e.g. `import {
        // EventEmitter } from "node:events"; EventEmitter.prototype`).
        // `lookup_native_module(name)` returns `(module, Some(symbol))`
        // for named imports and `(module, None)` for namespace imports
        // (`import * as events from "node:events"`). For named imports,
        // the member access is reading a property of that imported
        // *value*, not of the module namespace — so the appropriate
        // manifest entry to consult is the imported symbol itself
        // (which is already known to exist; that's how the import
        // resolved). Without this skip, every `EventEmitter.prototype`
        // / `Buffer.from(...).x` shape tripped the gate even when the
        // imported symbol was fully manifest-registered, because by
        // the time we're here the imported Ident has already been
        // value-form-lowered to `NativeModuleRef(module)` and the
        // original symbol name is no longer reachable from `object`.
        // Issue #859 followup: `test_issue_pino_prototype_undefined`
        // (the v0.5.938 #894 regression) hits exactly this with
        // `(EventEmitter as any).prototype`.
        let obj_is_named_import = match member.obj.as_ref() {
            ast::Expr::Ident(obj_ident) => matches!(
                ctx.lookup_native_module(obj_ident.sym.as_ref()),
                Some((_, Some(_)))
            ),
            // The `as any` / `as Foo` / `<T>x` casts wrap the Ident in
            // a TS-cast AST node before it reaches member access. Peel
            // them so the named-import detection survives the cast.
            ast::Expr::TsAs(ts_as) => match ts_as.expr.as_ref() {
                ast::Expr::Ident(obj_ident) => matches!(
                    ctx.lookup_native_module(obj_ident.sym.as_ref()),
                    Some((_, Some(_)))
                ),
                _ => false,
            },
            ast::Expr::TsNonNull(ts_nn) => match ts_nn.expr.as_ref() {
                ast::Expr::Ident(obj_ident) => matches!(
                    ctx.lookup_native_module(obj_ident.sym.as_ref()),
                    Some((_, Some(_)))
                ),
                _ => false,
            },
            ast::Expr::TsTypeAssertion(ts_ta) => match ts_ta.expr.as_ref() {
                ast::Expr::Ident(obj_ident) => matches!(
                    ctx.lookup_native_module(obj_ident.sym.as_ref()),
                    Some((_, Some(_)))
                ),
                _ => false,
            },
            ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                ast::Expr::Ident(obj_ident) => matches!(
                    ctx.lookup_native_module(obj_ident.sym.as_ref()),
                    Some((_, Some(_)))
                ),
                ast::Expr::TsAs(ts_as) => match ts_as.expr.as_ref() {
                    ast::Expr::Ident(obj_ident) => matches!(
                        ctx.lookup_native_module(obj_ident.sym.as_ref()),
                        Some((_, Some(_)))
                    ),
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        };
        if !obj_is_named_import
            && perry_api_manifest::module_has_any_entries(module)
            && perry_api_manifest::module_has_symbol(module, prop).is_none()
            // #wall4: a method that is unmistakably a `String.prototype` member
            // (`endsWith`, `startsWith`, `slice`, …) called on an identifier that
            // *happens* to share a node-core module name (`url`, `path`) means the
            // receiver is a runtime string value, NOT the module — don't gate it
            // as an unimplemented module API; fall through to a normal PropertyGet
            // so it dispatches dynamically on the real receiver. Next.js's
            // app-page-turbo bundle calls `url.endsWith(...)` on a URL *string*
            // bound to a local named `url`, which otherwise threw
            // "url.endsWith is not implemented in Perry (ahead-of-time)".
            && !super::super::array_fold::is_known_string_prototype_method(prop)
        {
            // #3896: a bare *value read* of an absent member on a Node
            // builtin module namespace/default object is an ordinary
            // property miss → `undefined` (e.g. `dns/promises.ADDRCONFIG`,
            // which Node also doesn't export but reads as undefined). Calls
            // (`ns.foo()`) keep going through the gate — `lower_call` set the
            // callee marker, so `member_is_call_callee` is true there. Only
            // Node core modules relax; unenumerated npm packages keep the gate.
            // This is independent of #463/#5245 strict-unimplemented mode (it's
            // a real Node semantic, not a degraded surface).
            if !member_is_call_callee && perry_api_manifest::is_node_core_module(module) {
                return Ok(Expr::Undefined);
            }
            // #925: when there's a known supported equivalent for this
            // shape, append it to the error so the user doesn't have to
            // grep through the manifest to find the replacement.
            let hint = super::super::unimpl_hints::module_member_hint(module, prop)
                .map(|h| format!(" {h}"))
                .unwrap_or_default();
            let msg = format!(
                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                module, prop, hint,
            );
            // #5245: defer to a throw-on-reach runtime error by default (record
            // for the end-of-compile notice); strict-unimplemented mode restores
            // the hard #463 refusal. #2309 tree-shake deferral is handled inside.
            let api = format!("{module}.{prop}");
            let location =
                crate::eval_classifier::location_string(&ctx.source_file_path, member.span.lo.0);
            match crate::check_unimplemented_api(&msg, &api, &location, member.span.lo.0) {
                crate::UnimplementedDecision::Refuse => {
                    crate::lower_bail!(member.span, "{}", msg);
                }
                crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                    return super::super::const_fold_fn::synth_deferred_throw_value(
                        ctx,
                        &runtime_msg,
                        member.span,
                    );
                }
            }
        }
    }

    match &member.prop {
        ast::MemberProp::Ident(ident) => {
            let property = ident.sym.to_string();
            Ok(Expr::PropertyGet {
                // #5247: carry the member access's source offset so a nullish
                // receiver ("Cannot read properties of undefined") localizes.
                byte_offset: member.span.lo.0,
                object,
                property,
            })
        }
        ast::MemberProp::Computed(computed) => {
            // #503: refuse compile-time dynamic dispatch on stdlib namespace
            // receivers — `process[runtimeVar]`, `fs[atob(...)]()`, etc. —
            // the dispatch-by-string class of supply-chain evasion. The check
            // runs on the AST so it sees the un-folded shape, and bails before
            // we lower the index (lowering can have side effects we want to
            // avoid for refused code).
            //
            // Only fires when:
            //   - the receiver AST is a bare ident naming a stdlib namespace
            //     (or an alias bound to one via `import x from 'fs'`),
            //   - the index is NOT a string literal at the source level
            //     (literal keys are caught by the fold below, and never
            //     constitute string-obfuscation),
            //   - the refusal pass is enabled — OFF by default since #5263,
            //     re-armed under `--lockdown` / `perry.lockdown` or the explicit
            //     opt-out `PERRY_ALLOW_DYNAMIC_STDLIB=0` /
            //     `perry.allowDynamicStdlibDispatch: false`,
            //   - the currently-lowering source file does NOT belong to a
            //     package on the per-package allow-list, and
            //   - there is no `// @perry-allow-dynamic` line annotation on
            //     or immediately above the offending site.
            // #1723: an enclosing `ns[dynamicKey].staticMember` access may have
            // marked THIS computed access as auditable sub-namespace selection.
            // Consume the one-shot flag (so a dynamic key in the index position
            // is still refused) and skip the refusal for exactly this access.
            let suppressed_by_parent = std::mem::take(&mut ctx.suppress_stdlib_dispatch_guard_once);
            if !suppressed_by_parent && crate::ir::refuse_dynamic_stdlib_dispatch_enabled() {
                if let Some(ns) = stdlib_namespace_receiver(ctx, member.obj.as_ref()) {
                    if !matches!(*computed.expr, ast::Expr::Lit(ast::Lit::Str(_))) {
                        let pkg = crate::ir::package_name_for_source_path(&ctx.source_file_path);
                        let pkg_allowed = pkg
                            .map(crate::ir::dynamic_stdlib_allowed_for_package)
                            .unwrap_or(false);
                        // #996: `// @perry-allow-dynamic` is host-code only.
                        // A malicious npm package can write the annotation next
                        // to its own call to defeat the refusal — closing the
                        // hole means dependencies must be opted in by the host
                        // via `perry.allowDynamicStdlibDispatch` (the
                        // `pkg_allowed` branch above), never by themselves.
                        let site_allowed = pkg.is_none()
                            && crate::ir::current_module_has_allow_dynamic_at(member.span.lo.0);
                        if !pkg_allowed && !site_allowed {
                            let pkg_label = pkg
                                .map(|p| format!(" (in package `{}`)", p))
                                .unwrap_or_default();
                            crate::lower_bail!(
                                member.span,
                                "dynamic dispatch on stdlib namespace `{}` is refused at \
                                 compile time{} — this catches the obfuscation pattern \
                                 `{}[runtimeVar]()` used by malicious npm packages. (#503)\n\
                                 \n\
                                 Options:\n\
                                 - Replace with a static call: `{}.<methodName>(...)`.\n\
                                 - If the indirection is intentional, add `// @perry-allow-dynamic` \
                                   on the line above the call.\n\
                                 - To opt an entire dependency out, add its name to \
                                   `perry.allowDynamicStdlibDispatch` in the host package.json, \
                                   or set `perry.allowDynamicStdlibDispatch: true` to disable \
                                   the check globally.\n\
                                 - Or set `PERRY_ALLOW_DYNAMIC_STDLIB=1` for a one-off build.",
                                ns,
                                pkg_label,
                                ns,
                                ns,
                            );
                        }
                    }
                }
            }

            let index = Box::new(lower_expr(ctx, &computed.expr)?);
            // Specialize for Uint8Array/Buffer variables → byte-level access.
            // Params declared `Buffer` (e.g. `function f(src: Buffer)`)
            // reach here with `Type::Named("Buffer")` — treat it as a
            // synonym for Uint8Array so `src[i]` uses the byte-read
            // path instead of the generic f64-element IndexGet, which
            // would return NaN-boxed pointer bits as a denormal f64.
            if let Expr::LocalGet(id) = &*object {
                if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                    // …but ONLY for a numeric key. A Buffer is an ordinary
                    // object in Node (a Uint8Array), so a STRING key reads a
                    // property, not a byte: `buf["writeInt8"]` is the method,
                    // `buf[k] = v` (k non-numeric) an expando. Folding those to
                    // the byte path returned `undefined` (an out-of-range byte
                    // read), which broke the ubiquitous feature-probe idiom
                    // `typeof obj[k] === "function"` — mysql2's `MockBuffer`
                    // uses it to neutralize the write methods of a zero-length
                    // Buffer while sizing a packet, so every outgoing MySQL
                    // packet was measured against a live (empty) buffer and the
                    // handshake died with RangeError [ERR_OUT_OF_RANGE].
                    let key_is_string = matches!(index.as_ref(), Expr::String(_))
                        || matches!(
                            index.as_ref(),
                            Expr::LocalGet(kid) if ctx
                                .locals
                                .iter()
                                .find(|(_, lid, _)| lid == kid)
                                .is_some_and(|(_, _, kty)| matches!(kty, Type::String))
                        );
                    if !key_is_string
                        && matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer")
                    {
                        return Ok(Expr::Uint8ArrayGet {
                            array: object,
                            index,
                        });
                    }
                }
            }
            // Issue #529: `obj["method"]` on a class instance with a static
            // string key is semantically equivalent to `obj.method` — both
            // forms must hit the same vtable dispatch. The dot form lowers
            // to `Expr::PropertyGet`, which codegen routes through
            // `js_class_method_bind` / vtable lookup; `IndexGet` on a class
            // instance falls through to the generic property-by-name read
            // (`js_dyn_index_get`), which only sees object fields and
            // returns undefined for methods. Fold static-string IndexGet
            // into PropertyGet so the two forms share a code path.
            //
            // Fold only when the index is a literal string that does NOT
            // parse as a non-negative integer — `arr["0"]` keeps IndexGet
            // semantics (string-coerced numeric element access on arrays).
            // This is the same disambiguator JavaScript's spec uses
            // internally for indexed-vs-named properties.
            if let Expr::String(key) = &*index {
                let is_numeric_string = !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_digit())
                    && !(key.len() > 1 && key.starts_with('0'));
                if !is_numeric_string {
                    return Ok(Expr::PropertyGet {
                        // #5247: `obj["prop"]` folds to a PropertyGet — carry the
                        // member offset so a nullish receiver localizes too.
                        byte_offset: member.span.lo.0,
                        object,
                        property: key.clone(),
                    });
                }
            }
            // `console[dynamicKey]` — the receiver is a bare `console` ident
            // (not shadowed: a local would have lowered `object` to a
            // LocalGet, not the `GlobalGet(0)` builtin sentinel). The static
            // `console.log` value read already resolves to a real bound
            // closure via `js_native_module_property_by_name`, but the
            // computed form fell through to `IndexGet { GlobalGet(0), key }`,
            // i.e. reading the method off numeric 0 — so `console[m](...)`
            // threw `(number).<m> is not a function` (the Next.js
            // `prefixedLog` wall). Route the runtime key through the same
            // native-module resolver so both forms agree.
            if matches!(&*object, Expr::GlobalGet(0))
                && matches!(member.obj.as_ref(), ast::Expr::Ident(id) if id.sym.as_ref() == "console")
            {
                return Ok(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_console_method_by_value".to_string(),
                        param_types: vec![Type::Any],
                        return_type: Type::Any,
                    }),
                    args: vec![*index],
                    type_args: Vec::new(),
                    byte_offset: 0,
                });
            }
            Ok(Expr::IndexGet { object, index })
        }
        ast::MemberProp::PrivateName(private) => {
            // Private field access: this.#field -> PropertyGet with "#field".
            // Wrap the receiver in a brand+kind guard so accessing the private
            // member on a wrong receiver throws TypeError per spec.
            let property = format!("#{}", private.name);
            let object = wrap_private_guard(ctx, object, &property, PRIV_OP_READ);
            Ok(Expr::PropertyGet {
                // #5247: `this.#field` — carry the member offset for nullish-receiver
                // localization (consistency with the public-property path).
                byte_offset: member.span.lo.0,
                object,
                property,
            })
        }
    }
}
