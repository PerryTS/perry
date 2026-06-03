//! Member expression lowering: `ast::Expr::Member`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 405-LOC `Member` arm
//! from `lower_expr`. Member expressions cover `obj.prop`,
//! `obj["key"]`, `obj[i]`, the namespace-form `Math.PI`, enum member
//! access (`Color.Red`), private field reads (`#field`), and a fast
//! path for `Symbol.iterator` / `Symbol.asyncIterator` / friends.
//! The arm is mostly a long match cascade: identify the receiver kind
//! (regular object vs class static vs enum vs builtin namespace) then
//! emit the right HIR variant.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};

pub(super) fn lower_member(ctx: &mut LoweringContext, member: &ast::MemberExpr) -> Result<Expr> {
    // #1723: when THIS access is the auditable `ns[dynamicKey].staticMember`
    // shape — a dynamic stdlib SUB-namespace selection (`path.win32` /
    // `path.posix`) followed by a source-visible static member — mark the
    // immediately-nested `ns[dynamicKey]` computed access as auditable so the
    // #503 refusal does not fire on it (the method/property name is in
    // plaintext, not the `ns[runtimeVar]()` obfuscation the guard targets). The
    // flag must be set HERE, before the many early-return arms below that lower
    // `member.obj` themselves — otherwise the receiver `ns[dynamicKey]` is
    // lowered through one of those arms and trips the guard. It is a one-shot
    // consumed by the first guarded access, so a dynamic key *inside the index*
    // (`ns[fs[evil]].x`) is still refused. Save/restore the prior value so the
    // flag never leaks past this member, and only touch it when our own
    // detection fires (a method-call receiver sets the flag via `lower_call`
    // instead, and that must survive into the bare `ns[dynamicKey]` lowering).
    let prev_unresolved_ident_as_global = ctx.unresolved_ident_as_global;
    ctx.unresolved_ident_as_global = true;
    let suppress_for_obj = stdlib_ns_subnamespace_static_access(ctx, member);
    if !suppress_for_obj {
        let result = lower_member_inner(ctx, member);
        ctx.unresolved_ident_as_global = prev_unresolved_ident_as_global;
        return result;
    }
    let prev_suppress = ctx.suppress_stdlib_dispatch_guard_once;
    ctx.suppress_stdlib_dispatch_guard_once = true;
    let result = lower_member_inner(ctx, member);
    ctx.suppress_stdlib_dispatch_guard_once = prev_suppress;
    ctx.unresolved_ident_as_global = prev_unresolved_ident_as_global;
    result
}

/// #3946: lower a value-read of a `node:process` core property imported by
/// name (`import { pid, arch } from "node:process"`) or read off a namespace
/// local. Mirrors the dedicated `process.<prop>` variants used by the global
/// member-access path so named/namespace forms agree with `process.<prop>`
/// instead of resolving to `undefined`. Methods (`cwd`, `exit`, …) return
/// `None` so the caller keeps lowering them to a callable native-module ref.
pub(crate) fn lower_process_named_property(prop: &str) -> Option<Expr> {
    Some(match prop {
        "argv" => Expr::ProcessArgv,
        "platform" => Expr::OsPlatform,
        "arch" => Expr::OsArch,
        "pid" => Expr::ProcessPid,
        "ppid" => Expr::ProcessPpid,
        "version" => Expr::ProcessVersion,
        "versions" => Expr::ProcessVersions,
        "env" => Expr::ProcessEnv,
        "stdin" => Expr::ProcessStdin,
        "stdout" => Expr::ProcessStdout,
        "stderr" => Expr::ProcessStderr,
        _ => return process_metadata_native_property(prop),
    })
}

fn process_native_property(prop: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::NativeModuleRef("process".to_string())),
        property: prop.to_string(),
    }
}

fn process_metadata_native_property(prop: &str) -> Option<Expr> {
    Some(match prop {
        "allowedNodeEnvironmentFlags"
        | "argv0"
        | "channel"
        | "config"
        | "connected"
        | "debugPort"
        | "disconnect"
        | "execArgv"
        | "execPath"
        | "features"
        | "finalization"
        | "moduleLoadList"
        | "permission"
        | "release"
        | "report"
        | "send"
        | "sourceMapsEnabled"
        | "title" => process_native_property(prop),
        _ => return None,
    })
}

fn ws_ready_state_value(prop: &str) -> Option<f64> {
    Some(match prop {
        "CONNECTING" => 0.0,
        "OPEN" => 1.0,
        "CLOSING" => 2.0,
        "CLOSED" => 3.0,
        _ => return None,
    })
}

fn is_ws_ready_state_receiver(
    ctx: &LoweringContext,
    obj_ast: &ast::Expr,
    object_expr: &Expr,
) -> bool {
    fn native_ws_class_property(expr: &Expr) -> bool {
        match expr {
            Expr::NativeModuleRef(module) if module == "ws" => true,
            Expr::PropertyGet { object, property }
                if matches!(property.as_str(), "WebSocket" | "default")
                    && matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "ws") =>
            {
                true
            }
            Expr::PropertyGet { object, property }
                if property == "WebSocket" && matches!(object.as_ref(), Expr::GlobalGet(0)) =>
            {
                true
            }
            _ => false,
        }
    }

    if native_ws_class_property(object_expr) {
        return true;
    }

    let ast::Expr::Ident(obj_ident) = obj_ast else {
        return false;
    };
    matches!(
        ctx.lookup_native_module(obj_ident.sym.as_ref()),
        Some(("ws", None | Some("default") | Some("WebSocket")))
    )
}

fn lower_member_inner(ctx: &mut LoweringContext, member: &ast::MemberExpr) -> Result<Expr> {
    // #3896: capture-and-clear the call-callee marker so it applies only to THIS
    // member (the immediate callee), not to nested member-object reads lowered
    // below. The #463 read-gate below uses `member_is_call_callee` to keep
    // rejecting `ns.foo()` while relaxing a bare `ns.foo` value read.
    let member_is_call_callee = ctx.lowering_call_callee;
    ctx.lowering_call_callee = false;
    // Issue #444: `import.meta.<prop>` folds directly to a literal at
    // lowering time. Routing through the bare-`import.meta` Object
    // synthesis hits a long-standing module-level NaN-boxing bug where
    // string fields read back as 0 — producing `url: 0` / `main: NaN`
    // for the user. Folding here sidesteps it entirely.
    //
    // Surface aligned with Node 20+ spec (`url` / `dirname` / `filename`
    // / `main`). Bun-only aliases (`dir` / `path` / `file`) intentionally
    // omitted — adding them would silently break code moving Perry → Node.
    if let ast::Expr::MetaProp(mp) = member.obj.as_ref() {
        if matches!(mp.kind, ast::MetaPropKind::ImportMeta) {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let (url, dirname, filename) = super::expr_misc::import_meta_paths(ctx);
                return Ok(match prop_ident.sym.as_ref() {
                    "url" => Expr::String(url),
                    "main" => Expr::Bool(ctx.is_entry_module),
                    "dirname" => Expr::String(dirname),
                    "filename" => Expr::String(filename),
                    // Unknown property — undefined matches the spec'd
                    // "missing property on a frozen object" behavior of
                    // import.meta in Node / Bun.
                    _ => Expr::Undefined,
                });
            }
        }
        // Issue #449: `new.target.<prop>` folds directly to a literal at
        // lowering time. The bare `MetaProp(NewTarget)` lowering in
        // `expr_misc::lower_meta_prop` returns an Object literal whose
        // string field reads back as the raw u64 handle bits (rendering
        // as `2e-323` / `NaN`) when constructed inside a class
        // constructor — same module-globals NaN-boxing bug class as
        // #444's `import.meta` Object. Folding the most common access
        // patterns here sidesteps it entirely. Inside a constructor,
        // `.name` is the class name string; outside, the whole
        // expression evaluates to `undefined.<prop>` which would throw
        // — but `new.target` outside a constructor is `undefined`, so
        // we lower the access to `Undefined` and let downstream
        // optional-chain rewrites (`new.target?.name`) handle the
        // null-guard correctly.
        if matches!(mp.kind, ast::MetaPropKind::NewTarget) {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                if let Some(class_name) = ctx.in_constructor_class.clone() {
                    return Ok(match prop_name {
                        "name" => Expr::String(class_name),
                        // Other props on a class reference (`prototype`,
                        // arbitrary) — undefined is the safe fallback;
                        // adding `prototype` would need a real class
                        // reference, not in scope for #449.
                        _ => Expr::Undefined,
                    });
                }
                // Outside a constructor: `new.target` is undefined and
                // ordinary functions resolve it dynamically at runtime.
                return Ok(Expr::PropertyGet {
                    object: Box::new(Expr::NewTarget),
                    property: prop_name.to_string(),
                });
            }
        }
    }

    // process.std{in,out,err}.{isTTY,columns,rows} — direct extern-call
    // shapes recognized BEFORE the regular process.X arm below, since the
    // double-Member shape (Member(Member(process, stream), prop)) doesn't
    // match the simple `process.X` Ident-then-prop dispatch. (#347 Phase 3.)
    if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
        if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
            if root_ident.sym.as_ref() == "process" && !ctx.shadows_unqualified_global("process") {
                if let (ast::MemberProp::Ident(stream_ident), ast::MemberProp::Ident(prop_ident)) =
                    (&inner_member.prop, &member.prop)
                {
                    let stream = stream_ident.sym.as_ref();
                    let prop = prop_ident.sym.as_ref();
                    match (stream, prop) {
                        ("stdin", "isTTY") => return Ok(Expr::ProcessStdinIsTTY),
                        ("stdout", "isTTY") => return Ok(Expr::ProcessStdoutIsTTY),
                        ("stderr", "isTTY") => return Ok(Expr::ProcessStderrIsTTY),
                        ("stdout", "columns") => return Ok(Expr::ProcessStdoutColumns),
                        ("stdout", "rows") => return Ok(Expr::ProcessStdoutRows),
                        _ => {}
                    }
                }
            }
        }
    }

    // Check if this is process.* property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        // #3946: the global `process`, and also a namespace/default import
        // local (`import * as p from "node:process"; p.pid` /
        // `import p from "node:process"; p.pid`) both route through the same
        // dedicated process-property lowering — otherwise the namespace form
        // fell through to a generic native-module PropertyGet that resolved
        // `pid`/`arch`/`platform`/… to `undefined`.
        let obj_name = obj_ident.sym.as_ref();
        let process_name_is_shadowed =
            obj_name == "process" && ctx.shadows_unqualified_global("process");
        let is_process_obj = !process_name_is_shadowed
            && (obj_name == "process"
                || matches!(
                    ctx.lookup_native_module(obj_name),
                    Some(("process", None)) | Some(("process.namespace", None))
                ));
        if is_process_obj {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop = prop_ident.sym.as_ref();
                if let Some(expr) = process_metadata_native_property(prop) {
                    return Ok(expr);
                }
                match prop {
                    "argv" => return Ok(Expr::ProcessArgv),
                    "platform" => return Ok(Expr::OsPlatform),
                    "arch" => return Ok(Expr::OsArch),
                    "pid" => return Ok(Expr::ProcessPid),
                    "ppid" => return Ok(Expr::ProcessPpid),
                    "version" => return Ok(Expr::ProcessVersion),
                    "versions" => return Ok(Expr::ProcessVersions),
                    "stdin" => return Ok(Expr::ProcessStdin),
                    "stdout" => return Ok(Expr::ProcessStdout),
                    "stderr" => return Ok(Expr::ProcessStderr),
                    "env" => return Ok(Expr::ProcessEnv),
                    // #1349: process.execArgv is the array of runtime CLI
                    // flags the interpreter was started with (`["--inspect",
                    // ...]` for Node). Perry binaries are AOT — there's no
                    // runtime flag list to forward — so the empty array is
                    // the correct shape. Without this, the bare read
                    // returns a 0 sentinel and `Array.isArray(...)` /
                    // `.length` / iteration all explode.
                    "execArgv" => return Ok(Expr::Array(Vec::new())),
                    // #1348: process.release — object describing the
                    // current runtime release. Node returns at least
                    // `{ name, sourceUrl, headersUrl }`. Perry binaries
                    // are AOT and shouldn't pretend to be a Node download
                    // tarball, but consumers feature-detect on
                    // `process.release.name === "node"`, so we match that
                    // shape with empty source/headers URLs.
                    "release" => {
                        return Ok(Expr::Object(vec![
                            ("name".to_string(), Expr::String("node".to_string())),
                            ("sourceUrl".to_string(), Expr::String(String::new())),
                            ("headersUrl".to_string(), Expr::String(String::new())),
                        ]));
                    }
                    // #1378: process.features — object of boolean capability
                    // flags. Consumers feature-detect on individual fields
                    // (e.g. `process.features.openssl_is_boringssl`); a bare
                    // read of `process.features` previously returned a 0
                    // sentinel, so `.X` on it was always undefined. Lower
                    // to an inline object literal matching the Node shape.
                    // All Perry flags are `false` except `ipv6` (the
                    // runtime's `node:dgram`/network stack handles it) —
                    // the literal mirrors what we actually link in.
                    "features" => return Ok(process_features_literal()),
                    // #1400 / #3108: process.sourceMapsEnabled — live boolean
                    // reflecting setSourceMapsEnabled(). Perry compiles AOT
                    // and ships no source-map resolver, so the flag drives
                    // nothing observable, but it round-trips through the
                    // setter (starting `false`) so `typeof ... === "boolean"`
                    // holds and toggles are observable. Lower to a 0-arg
                    // native getter that reads the runtime flag.
                    "sourceMapsEnabled" => {
                        return Ok(Expr::NativeMethodCall {
                            module: "process".to_string(),
                            class_name: None,
                            object: None,
                            method: "sourceMapsEnabled".to_string(),
                            args: Vec::new(),
                        })
                    }
                    // #1412: `process.moduleLoadList` is Node's list of
                    // built-in modules already loaded into the
                    // interpreter. Perry AOT-compiles every reachable
                    // module into the binary — there is no runtime
                    // module loader and no observable "load list", so
                    // the spec-compatible value is an empty array. Code
                    // that probes the shape (Array.isArray, .length,
                    // .includes(name)) now does the right thing instead
                    // of crashing on the 0.0 sentinel.
                    "moduleLoadList" => return Ok(Expr::Array(vec![])),
                    "finalization" => return Ok(process_native_property("finalization")),
                    // #1379: process.config — object describing build-time
                    // config (`{ variables, target_defaults }` in Node).
                    // Perry has no `node-gyp`-style build to surface, but
                    // consumers feature-detect on `process.config.variables`
                    // existing (or specific fields like `target_arch`), so
                    // return the shape with empty sub-objects rather than
                    // letting the bare read fall through to the 0 sentinel.
                    "config" => {
                        return Ok(Expr::Object(vec![
                            ("variables".to_string(), Expr::Object(Vec::new())),
                            ("target_defaults".to_string(), Expr::Object(Vec::new())),
                        ]));
                    }
                    // #1380 / #2589: process.allowedNodeEnvironmentFlags —
                    // the Set of NODE_OPTIONS / V8 flags Node accepts from
                    // the environment. Perry binaries are AOT and don't
                    // honour NODE_OPTIONS-style runtime flags, but consumers
                    // feature-detect on this being a real, non-empty `Set`
                    // (`instanceof Set`, `.size > 0`, `.has("--no-warnings")`,
                    // iteration), so materialise it with a Node-compatible
                    // flag list rather than the previously-empty Set.
                    "allowedNodeEnvironmentFlags" => {
                        return Ok(process_allowed_node_flags_literal())
                    }
                    "report" => return Ok(process_native_property("report")),
                    // #1346: process.argv0 / execPath / title — Node
                    // documents these as strings (program-invocation
                    // name / resolved-binary path / OS-displayed
                    // title). Perry was hitting the 0.0 sentinel and
                    // `typeof process.argv0 === "string"` failed; any
                    // `.length` / `.endsWith(...)` then crashed.
                    //
                    // Lower all three to `process.argv[0]` — Perry's
                    // own argv[0] is already the binary path / name
                    // we'd want for argv0 and execPath, and is a
                    // reasonable default for `title` (Node defaults
                    // to argv[0] too until something assigns `.title`).
                    // Settable `process.title` is tracked separately
                    // (#1401); the shape-only read is what closes #1346.
                    "argv0" | "execPath" => {
                        return Ok(Expr::IndexGet {
                            object: Box::new(Expr::ProcessArgv),
                            index: Box::new(Expr::Number(0.0)),
                        });
                    }
                    "title" => {
                        // #1401: title is settable; route through a
                        // runtime cell that falls back to argv[0].
                        return Ok(Expr::ProcessTitle);
                    }
                    // #1350: process.exitCode value-read. Default is
                    // `undefined` until something assigns to it; after a
                    // write the previously-stored value round-trips. The
                    // assignment side intercepts `process.exitCode = v`
                    // in `lower_expr.rs` and routes to
                    // `js_process_exit_code_set`. Both helpers share a
                    // thread-local cell in `perry-runtime/src/process.rs`.
                    "exitCode" => {
                        return Ok(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: "js_process_exit_code_get".to_string(),
                                param_types: vec![],
                                return_type: Type::Number,
                            }),
                            args: vec![],
                            type_args: vec![],
                        });
                    }
                    _ => {}
                }
                // #1343: a `process.<method>` read used as a VALUE. The
                // call form (`process.cwd()`) is intercepted in expr_call
                // and lowered to its dedicated `ProcessCwd`/etc. variant
                // before reaching here, so this only fires for bare reads
                // (`typeof process.cwd`, `const f = process.cwd`). The arms
                // above cover process *properties* (argv/env/pid/…); anything
                // the API manifest classifies as a process *method* is a
                // callable function value in Node. Lower it to a
                // `NativeModuleRef("process")` property read so the codegen
                // typeof short-circuit (which consults `module_has_symbol`)
                // reports "function" — exactly the already-working
                // `crypto.<method>` namespace path. Without this, `process`
                // lowers to a `GlobalGet` and `typeof process.cwd` read
                // "undefined" even though `process.cwd()` works.
                let prop = prop_ident.sym.as_ref();
                if matches!(
                    perry_api_manifest::module_has_symbol("process", prop).map(|e| &e.kind),
                    Some(perry_api_manifest::ApiKind::Method { .. })
                ) {
                    return Ok(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef("process".to_string())),
                        property: prop.to_string(),
                    });
                }
            }
        }
        // `globalThis.process` returns an object whose `.env`/`.argv`/
        // etc. should resolve just like bare `process.*`. Without this
        // shim, `globalThis.process.env` walks through generic
        // PropertyGet dispatch and hits a 0.0 sentinel. Matches the
        // static `process.env` fast path above.
        if obj_ident.sym.as_ref() == "globalThis" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                if prop_ident.sym.as_ref() == "process" {
                    // `globalThis.process` on its own — fall through
                    // to generic handling below (returns 0.0 sentinel,
                    // which is fine as the outer chain handles env/etc.).
                }
            }
        }
    }
    // Handle `globalThis.process.X` (and any PropertyGet whose object
    // resolves to `globalThis.process`): treat the outer `.X` as if
    // it were a bare `process.X` access. Unwraps transparent TS
    // wrappers (TsAs, TsNonNull, TsSatisfies, TsTypeAssertion, Paren)
    // so that `(globalThis as any).process.env` works too.
    fn unwrap_transparent(e: &ast::Expr) -> &ast::Expr {
        let mut cur = e;
        loop {
            match cur {
                ast::Expr::TsAs(x) => cur = &x.expr,
                ast::Expr::TsNonNull(x) => cur = &x.expr,
                ast::Expr::TsSatisfies(x) => cur = &x.expr,
                ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
                ast::Expr::TsConstAssertion(x) => cur = &x.expr,
                ast::Expr::Paren(x) => cur = &x.expr,
                _ => return cur,
            }
        }
    }
    let member_obj_unwrapped = unwrap_transparent(member.obj.as_ref());
    if let ast::Expr::Member(inner) = member_obj_unwrapped {
        let inner_obj_unwrapped = unwrap_transparent(inner.obj.as_ref());
        let inner_is_global_process = matches!(
            inner_obj_unwrapped,
            ast::Expr::Ident(i) if i.sym.as_ref() == "globalThis"
        ) && matches!(
            &inner.prop,
            ast::MemberProp::Ident(p) if p.sym.as_ref() == "process"
        );
        if inner_is_global_process {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop = prop_ident.sym.as_ref();
                if let Some(expr) = process_metadata_native_property(prop) {
                    return Ok(expr);
                }
                match prop {
                    "argv" => return Ok(Expr::ProcessArgv),
                    "platform" => return Ok(Expr::OsPlatform),
                    "arch" => return Ok(Expr::OsArch),
                    "pid" => return Ok(Expr::ProcessPid),
                    "ppid" => return Ok(Expr::ProcessPpid),
                    "version" => return Ok(Expr::ProcessVersion),
                    "versions" => return Ok(Expr::ProcessVersions),
                    "env" => return Ok(Expr::ProcessEnv),
                    "execArgv" => return Ok(Expr::Array(Vec::new())),
                    "release" => {
                        return Ok(Expr::Object(vec![
                            ("name".to_string(), Expr::String("node".to_string())),
                            ("sourceUrl".to_string(), Expr::String(String::new())),
                            ("headersUrl".to_string(), Expr::String(String::new())),
                        ]));
                    }
                    "features" => return Ok(process_features_literal()),
                    // #3108: live boolean toggle — see the matching arm above.
                    "sourceMapsEnabled" => {
                        return Ok(Expr::NativeMethodCall {
                            module: "process".to_string(),
                            class_name: None,
                            object: None,
                            method: "sourceMapsEnabled".to_string(),
                            args: Vec::new(),
                        })
                    }
                    "moduleLoadList" => return Ok(Expr::Array(vec![])),
                    "finalization" => return Ok(process_native_property("finalization")),
                    "config" => {
                        return Ok(Expr::Object(vec![
                            ("variables".to_string(), Expr::Object(Vec::new())),
                            ("target_defaults".to_string(), Expr::Object(Vec::new())),
                        ]));
                    }
                    "allowedNodeEnvironmentFlags" => {
                        return Ok(process_allowed_node_flags_literal())
                    }
                    "report" => return Ok(process_native_property("report")),
                    "argv0" | "execPath" => {
                        return Ok(Expr::IndexGet {
                            object: Box::new(Expr::ProcessArgv),
                            index: Box::new(Expr::Number(0.0)),
                        });
                    }
                    "title" => return Ok(Expr::ProcessTitle),
                    "exitCode" => {
                        return Ok(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: "js_process_exit_code_get".to_string(),
                                param_types: vec![],
                                return_type: Type::Number,
                            }),
                            args: vec![],
                            type_args: vec![],
                        });
                    }
                    "on"
                    | "addListener"
                    | "once"
                    | "prependListener"
                    | "prependOnceListener"
                    | "emit"
                    | "listeners"
                    | "rawListeners"
                    | "eventNames"
                    | "listenerCount"
                    | "removeListener"
                    | "off"
                    | "removeAllListeners"
                    | "setMaxListeners"
                    | "getMaxListeners" => {
                        return Ok(Expr::PropertyGet {
                            object: Box::new(Expr::GlobalGet(0)),
                            property: prop_ident.sym.to_string(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // Check if this is Symbol.<well-known> — Symbol.toPrimitive,
    // Symbol.hasInstance, Symbol.toStringTag, Symbol.match, Symbol.iterator,
    // Symbol.asyncIterator, Symbol.dispose, Symbol.asyncDispose, and the
    // remaining standard protocol constants.
    // Lowered to `SymbolFor(String("@@__perry_wk_<name>"))` which the
    // runtime's `js_symbol_for` sniffs via prefix and resolves from
    // the well-known cache (not the registry). Gives each well-known
    // symbol a stable pointer without needing a new HIR variant.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Symbol" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                if matches!(
                    prop_name,
                    "toPrimitive"
                        | "hasInstance"
                        | "toStringTag"
                        | "species"
                        | "match"
                        | "matchAll"
                        | "replace"
                        | "search"
                        | "split"
                        | "isConcatSpreadable"
                        | "unscopables"
                        | "iterator"
                        | "asyncIterator"
                        | "dispose"
                        | "asyncDispose"
                ) {
                    return Ok(Expr::SymbolFor(Box::new(Expr::String(format!(
                        "@@__perry_wk_{}",
                        prop_name
                    )))));
                }
            }
        }
    }

    // `util.inspect.custom` / `inspect.custom` and
    // `util.promisify.custom` / `promisify.custom` (named imports from
    // node:util) — Node exposes these as registered `Symbol.for(...)`
    // values, and computed keys expect those exact descriptions.
    // See #1201 and util.promisify.custom parity.
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if prop_ident.sym.as_ref() == "custom" {
            // Case A: `inspect.custom` where `inspect` is a named import from
            // node:util, and the analogous `promisify.custom`.
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if let Some((module_name, Some(method_name))) =
                    ctx.lookup_native_module(obj_ident.sym.as_ref())
                {
                    if module_name == "util" || module_name == "node:util" {
                        if method_name == "inspect" {
                            return Ok(Expr::SymbolFor(Box::new(Expr::String(
                                "nodejs.util.inspect.custom".to_string(),
                            ))));
                        }
                        if method_name == "promisify" {
                            return Ok(Expr::SymbolFor(Box::new(Expr::String(
                                "nodejs.util.promisify.custom".to_string(),
                            ))));
                        }
                    }
                }
            }
            // Case B: `util.inspect.custom` where `util` is a whole-module
            // alias (`import * as util from "node:util"` or
            // `import util from "node:util"`), and the analogous
            // `util.promisify.custom`.
            if let ast::Expr::Member(inner) = member.obj.as_ref() {
                if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(inner_prop)) =
                    (inner.obj.as_ref(), &inner.prop)
                {
                    let obj_name = obj_ident.sym.to_string();
                    let is_util_module = obj_name == "util"
                        || ctx.lookup_builtin_module_alias(&obj_name) == Some("util");
                    if is_util_module {
                        match inner_prop.sym.as_ref() {
                            "inspect" => {
                                return Ok(Expr::SymbolFor(Box::new(Expr::String(
                                    "nodejs.util.inspect.custom".to_string(),
                                ))));
                            }
                            "promisify" => {
                                return Ok(Expr::SymbolFor(Box::new(Expr::String(
                                    "nodejs.util.promisify.custom".to_string(),
                                ))));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Check if this is path.sep / path.delimiter constant access
    // (where `path` is an imported alias of the node:path module).
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        let is_path_module =
            obj_name == "path" || ctx.lookup_builtin_module_alias(&obj_name) == Some("path");
        if is_path_module {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                match prop_ident.sym.as_ref() {
                    "sep" => return Ok(Expr::PathSep),
                    "delimiter" => return Ok(Expr::PathDelimiter),
                    _ => {}
                }
            }
        }
    }

    // path.win32.sep / path.win32.delimiter (and path.posix.sep/.delimiter)
    // — sub-namespace constants. Lower directly to string literals; no
    // runtime call needed (issue #1162).
    if let ast::Expr::Member(inner) = member.obj.as_ref() {
        if let (ast::Expr::Ident(root_ident), ast::MemberProp::Ident(sub_prop)) =
            (inner.obj.as_ref(), &inner.prop)
        {
            let root_name = root_ident.sym.to_string();
            let is_path_root =
                root_name == "path" || ctx.lookup_builtin_module_alias(&root_name) == Some("path");
            if is_path_root {
                let sub = sub_prop.sym.as_ref();
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let prop = prop_ident.sym.as_ref();
                    match (sub, prop) {
                        ("win32", "sep") => return Ok(Expr::String("\\".to_string())),
                        ("win32", "delimiter") => return Ok(Expr::String(";".to_string())),
                        ("posix", "sep") => return Ok(Expr::String("/".to_string())),
                        ("posix", "delimiter") => return Ok(Expr::String(":".to_string())),
                        _ => {}
                    }
                }
            }
        }
    }

    // Check if this is a process.env.VARNAME or process.env[expr] access
    if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
        if let ast::Expr::Ident(obj_ident) = inner_member.obj.as_ref() {
            if obj_ident.sym.as_ref() == "process" {
                if let ast::MemberProp::Ident(prop_ident) = &inner_member.prop {
                    if prop_ident.sym.as_ref() == "env" {
                        // This is process.env access
                        match &member.prop {
                            ast::MemberProp::Ident(var_ident) => {
                                // process.env.VARNAME (static key)
                                let var_name = var_ident.sym.to_string();
                                return Ok(Expr::EnvGet(var_name));
                            }
                            ast::MemberProp::Computed(computed) => {
                                // process.env["NAME"] with a string-literal key
                                // is a *static* access — lower to EnvGet so it
                                // matches the dot form (and #2309 build-time
                                // folding sees it). Non-literal keys stay
                                // dynamic.
                                if let ast::Expr::Lit(ast::Lit::Str(s)) = computed.expr.as_ref() {
                                    if let Some(name) = s.value.as_str() {
                                        return Ok(Expr::EnvGet(name.to_string()));
                                    }
                                }
                                // process.env[expr] (dynamic key)
                                let key_expr = Box::new(lower_expr(ctx, &computed.expr)?);
                                return Ok(Expr::EnvGetDynamic(key_expr));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Check for Math constants (e.g., Math.PI, Math.E)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Math" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let val = match prop_ident.sym.as_ref() {
                    "PI" => Some(std::f64::consts::PI),
                    "E" => Some(std::f64::consts::E),
                    "LN2" => Some(std::f64::consts::LN_2),
                    "LN10" => Some(std::f64::consts::LN_10),
                    "LOG2E" => Some(std::f64::consts::LOG2_E),
                    "LOG10E" => Some(std::f64::consts::LOG10_E),
                    "SQRT2" => Some(std::f64::consts::SQRT_2),
                    "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
                    _ => None,
                };
                if let Some(v) = val {
                    return Ok(Expr::Number(v));
                }
            }
        }
    }

    // Check for Number constants (e.g., Number.MAX_SAFE_INTEGER)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Number" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let val = match prop_ident.sym.as_ref() {
                    "MAX_SAFE_INTEGER" => Some(9007199254740991.0),
                    "MIN_SAFE_INTEGER" => Some(-9007199254740991.0),
                    "MAX_VALUE" => Some(f64::MAX),
                    "MIN_VALUE" => Some(f64::MIN_POSITIVE),
                    "EPSILON" => Some(f64::EPSILON),
                    "POSITIVE_INFINITY" => Some(f64::INFINITY),
                    "NEGATIVE_INFINITY" => Some(f64::NEG_INFINITY),
                    "NaN" => Some(f64::NAN),
                    _ => None,
                };
                if let Some(v) = val {
                    return Ok(Expr::Number(v));
                }
            }
        }
    }

    // #2902: `<TypedArray>.BYTES_PER_ELEMENT` static property — fold to the
    // element byte width. Works for all the global typed-array constructors
    // (Int8Array..Float64Array, including Float16Array=2). Only fires when the
    // name is a real global typed-array ctor not shadowed by a local binding.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.as_ref();
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            if prop_ident.sym.as_ref() == "BYTES_PER_ELEMENT" {
                if let Some(kind) = crate::ir::typed_array_kind_for_name(obj_name) {
                    let shadowed = ctx.lookup_local(obj_name).is_some()
                        || ctx.lookup_func(obj_name).is_some()
                        || ctx.lookup_imported_func(obj_name).is_some()
                        || ctx.lookup_class(obj_name).is_some();
                    if !shadowed {
                        let bytes = match kind {
                            crate::ir::TYPED_ARRAY_KIND_INT8
                            | crate::ir::TYPED_ARRAY_KIND_UINT8
                            | crate::ir::TYPED_ARRAY_KIND_UINT8_CLAMPED => 1.0,
                            crate::ir::TYPED_ARRAY_KIND_INT16
                            | crate::ir::TYPED_ARRAY_KIND_UINT16
                            | crate::ir::TYPED_ARRAY_KIND_FLOAT16 => 2.0,
                            crate::ir::TYPED_ARRAY_KIND_INT32
                            | crate::ir::TYPED_ARRAY_KIND_UINT32
                            | crate::ir::TYPED_ARRAY_KIND_FLOAT32 => 4.0,
                            _ => 8.0, // Float64 / BigInt64 / BigUint64
                        };
                        return Ok(Expr::Number(bytes));
                    }
                }
            }
        }
    }

    // Check if this is an enum member access (e.g., Color.Red)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if ctx.lookup_enum(&obj_name).is_some() {
            // This is an enum access
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let member_name = prop_ident.sym.to_string();
                return Ok(Expr::EnumMember {
                    enum_name: obj_name,
                    member_name,
                });
            }
        }
    }

    // Check if this is a static field access (e.g., Counter.count)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if ctx.lookup_class(&obj_name).is_some() {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let field_name = prop_ident.sym.to_string();
                if ctx.has_static_field(&obj_name, &field_name) {
                    return Ok(Expr::StaticFieldGet {
                        class_name: obj_name,
                        field_name,
                    });
                }
            }
        }
    }

    // Check if this is a namespace variable access (e.g., Flag.OPENCODE_AUTO_SHARE)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            let member_name = prop_ident.sym.to_string();
            if let Some(local_id) = ctx.lookup_namespace_var(&obj_name, &member_name) {
                return Ok(Expr::LocalGet(local_id));
            }
        }
    }

    // Check if this is os.EOL / os.devNull property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.as_ref();
        let is_os_module =
            obj_name == "os" || ctx.lookup_builtin_module_alias(obj_name) == Some("os");
        if is_os_module {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                match prop_ident.sym.as_ref() {
                    "EOL" => return Ok(Expr::OsEOL),
                    "devNull" => return Ok(Expr::OsDevNull),
                    _ => {}
                }
            }
        }
    }

    // --- Proxy property get: `p.foo` / `p[k]` for known proxy locals ---
    {
        fn unwrap_member_obj(mut e: &ast::Expr) -> &ast::Expr {
            loop {
                match e {
                    ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                    ast::Expr::TsNonNull(nn) => e = &nn.expr,
                    ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                    ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                    ast::Expr::Paren(p) => e = &p.expr,
                    _ => break,
                }
            }
            e
        }
        let inner = unwrap_member_obj(member.obj.as_ref());
        if let ast::Expr::Ident(obj_ident) = inner {
            let obj_name = obj_ident.sym.to_string();
            if ctx.proxy_locals.contains(&obj_name) {
                let proxy_expr = if let Some(id) = ctx.lookup_local(&obj_name) {
                    Expr::LocalGet(id)
                } else {
                    lower_expr(ctx, &member.obj)?
                };
                let key_expr = match &member.prop {
                    ast::MemberProp::Ident(i) => Expr::String(i.sym.to_string()),
                    ast::MemberProp::Computed(c) => lower_expr(ctx, &c.expr)?,
                    ast::MemberProp::PrivateName(pn) => {
                        Expr::String(format!("#{}", pn.name.as_str()))
                    }
                };
                return Ok(Expr::ProxyGet {
                    proxy: Box::new(proxy_expr),
                    key: Box::new(key_expr),
                });
            }
        }
    }

    // Issue #838 followup (b) — read side: `<funcDecl>.prototype.<name>`
    // (and the computed-string-literal form
    // `<funcDecl>.prototype['<name>']`). The assignment side routes
    // through `Expr::RegisterFunctionPrototypeMethod` which stores the
    // method in `CLASS_PROTOTYPE_METHODS[synthetic_cid]`; pre-fix the
    // matching read fell through to `PropertyGet(PropertyGet(funcDecl,
    // "prototype"), name)` whose receiver evaluated to `undefined`, so
    // `typeof Foo.prototype.method` came back `'undefined'` even with a
    // working dispatch. Look up the side-table directly here. Same
    // unwrap helper as the assignment-side recogniser so TS casts
    // (`(Foo.prototype as any).method`) don't defeat the match.
    {
        fn unwrap_ts_local(e: &ast::Expr) -> &ast::Expr {
            let mut cur = e;
            loop {
                match cur {
                    ast::Expr::TsAs(x) => cur = &x.expr,
                    ast::Expr::TsNonNull(x) => cur = &x.expr,
                    ast::Expr::TsSatisfies(x) => cur = &x.expr,
                    ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
                    ast::Expr::TsConstAssertion(x) => cur = &x.expr,
                    ast::Expr::Paren(x) => cur = &x.expr,
                    _ => return cur,
                }
            }
        }
        let method_name_opt: Option<String> = match &member.prop {
            ast::MemberProp::Ident(p) => Some(p.sym.to_string()),
            ast::MemberProp::Computed(c) => match c.expr.as_ref() {
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    Some(s.value.as_str().unwrap_or("").to_string())
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(method_name) = method_name_opt {
            let obj_unwrapped = unwrap_ts_local(member.obj.as_ref());
            if let ast::Expr::Member(inner) = obj_unwrapped {
                let prop_is_prototype = matches!(
                    &inner.prop,
                    ast::MemberProp::Ident(p) if p.sym.as_ref() == "prototype"
                );
                if prop_is_prototype {
                    let inner_obj = unwrap_ts_local(inner.obj.as_ref());
                    if let ast::Expr::Ident(fn_ident) = inner_obj {
                        let fn_name = fn_ident.sym.to_string();
                        // Mirror the assignment-side resolution order:
                        // function-typed local > top-level FuncRef. Skip
                        // classes — `class C` already has a real proto
                        // object exposed elsewhere and the side-table
                        // walk wouldn't help here. Skip native imports
                        // since their `.prototype` is module-managed.
                        if ctx.lookup_class(&fn_name).is_none()
                            && !matches!(ctx.lookup_native_module(&fn_name), Some((_, Some(_))))
                        {
                            let func_expr = if let Some(local_id) = ctx.lookup_local(&fn_name) {
                                if ctx.function_valued_locals.contains(&local_id) {
                                    Some(Expr::LocalGet(local_id))
                                } else {
                                    None
                                }
                            } else if let Some(func_id) = ctx.lookup_func(&fn_name) {
                                Some(Expr::FuncRef(func_id))
                            } else {
                                None
                            };
                            if let Some(func_expr) = func_expr {
                                return Ok(Expr::GetFunctionPrototypeMethod {
                                    func: Box::new(func_expr),
                                    method_name,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // #1531/#1532: Node stream instances expose the normal JS constructor
    // function (`r.constructor === Readable`, `r.constructor.name ===
    // "Readable"`). Native-instance value reads normally lower to a 0-arg
    // NativeMethodCall/getter, but `constructor` is metadata, not a stream
    // method. Lower it back to the named module export so typeof/name reads
    // see the callable constructor.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            if prop_ident.sym.as_ref() == "constructor" {
                if let Some(("stream", class_name)) =
                    ctx.lookup_native_instance(obj_ident.sym.as_ref())
                {
                    if matches!(
                        class_name,
                        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                    ) {
                        return Ok(Expr::PropertyGet {
                            object: Box::new(Expr::NativeModuleRef("stream".to_string())),
                            property: class_name.to_string(),
                        });
                    }
                }
            }
        }
    }

    // Check for native instance property access (e.g., response.status, response.ok)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        // Clone module_name + class_name early to avoid borrow issues.
        // Issue #577 — preserve class_name in the lowered NativeMethodCall
        // so the codegen NATIVE_MODULE_TABLE class_filter dispatch fires
        // for getters like `req.method` / `res.statusCode` that have
        // class_filter = Some("IncomingMessage" / "ServerResponse").
        let native_instance = ctx
            .lookup_native_instance(&obj_name)
            .map(|(m, c)| (m.to_string(), c.to_string()));
        if let Some((module_name, class_name)) = native_instance {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let property_name = prop_ident.sym.to_string();
                // Issue #562: stream subclass instances (e.g.
                // `class W extends WritableStream`) carry the bare-stream
                // module/class tag for inherited-method dispatch
                // (`w.pipeTo(...)` / `w.getWriter()`), but they ALSO
                // declare their own fields (`w.seenLengths` / `w.config`).
                // Without this gate, every plain field read would route
                // through the NativeMethodCall arm in `lower_call.rs`,
                // miss the streams' known-method match, fall through to
                // the receiver-less zero-sentinel, and read as 0. Only
                // route to NativeMethodCall when the property name is a
                // known stream API method/property — let everything else
                // fall through to regular object property access.
                if matches!(
                    module_name.as_str(),
                    "readable_stream"
                        | "writable_stream"
                        | "transform_stream"
                        | "readable_stream_reader"
                        | "writable_stream_writer"
                ) && !is_stream_api_member(&module_name, &property_name)
                {
                    // Fall through — let the regular member access path
                    // below handle the user-declared subclass field.
                } else if matches!(module_name.as_str(), "util" | "sys")
                    && matches!(class_name.as_str(), "MIMEType" | "MIMEParams")
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "url"
                    && class_name == "URLPattern"
                    && is_url_pattern_data_property(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "worker_threads"
                    && matches!(class_name.as_str(), "MessagePort" | "BroadcastChannel")
                    && matches!(
                        property_name.as_str(),
                        "postMessage"
                            | "close"
                            | "ref"
                            | "unref"
                            | "hasRef"
                            | "addEventListener"
                            | "removeEventListener"
                    )
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "stream" && is_classic_stream_method_name(&property_name) {
                    // Classic Node streams materialize core stream and
                    // EventEmitter methods as closure-valued fields on the
                    // stream object. A bare method read (`r.read`, `r.on`)
                    // must return that callable value, not invoke the native
                    // receiver method with no args.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "net"
                    && matches!(class_name.as_str(), "Socket" | "Stream")
                    && is_net_socket_method_name(&property_name)
                {
                    // `new net.Socket().write` / `net.Stream().destroy` are
                    // method-value reads, not zero-arg native calls. Keep the
                    // PropertyGet shape so runtime handle-property dispatch
                    // can bind a callable to the socket handle.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "events"
                    && (matches!(
                        class_name.as_str(),
                        "EventEmitter" | "EventEmitterAsyncResource"
                    ) && (matches!(
                        property_name.as_str(),
                        "on" | "addListener"
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
                    ) || (class_name == "EventEmitterAsyncResource"
                        && property_name == "emitDestroy")))
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "console"
                    && class_name == "Console"
                    && is_console_instance_method_name(&property_name)
                {
                    // `new Console(...).log` is a method value read, not a
                    // zero-arg native getter. The call form still lowers
                    // through NativeMethodCall; bare reads stay as PropertyGet
                    // so runtime lookup can return a bound callable.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "Agent"
                    && matches!(
                        property_name.as_str(),
                        "keepSocketAlive" | "reuseSocket" | "getName" | "destroy" | "close"
                    )
                {
                    // A bare read of an Agent method (`typeof a.getName`)
                    // should produce a callable bound-method value, not invoke
                    // the native method with zero arguments.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && matches!(class_name.as_str(), "HttpServer" | "HttpsServer")
                    && matches!(
                        property_name.as_str(),
                        "listen"
                            | "close"
                            | "closeAllConnections"
                            | "closeIdleConnections"
                            | "on"
                            | "addListener"
                            | "address"
                            | "setTimeout"
                    )
                {
                    // Bare reads of HTTP/HTTPS server method values
                    // (`typeof server.listen`) return a callable bound method
                    // rather than invoking the native method with zero args.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "dns" | "dns/promises")
                    && class_name == "Resolver"
                    && is_dns_resolver_method_name(&property_name)
                {
                    // `dns.Resolver`/`dns/promises.Resolver` instances expose
                    // callable method-valued fields. A bare method read
                    // (`typeof r.resolve4`) returns that closure rather than
                    // invoking the receiver stub as a 0-arg getter.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "dgram"
                    && class_name == "Socket"
                    && is_dgram_socket_method_name(&property_name)
                {
                    // `dgram.createSocket()` returns a socket-shaped stub
                    // object whose methods are callable fields. A bare method
                    // read (`typeof s.close`) should observe that closure
                    // instead of invoking the receiver stub as a getter.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "inspector" | "inspector/promises")
                    && class_name == "Session"
                    && matches!(
                        property_name.as_str(),
                        "connect" | "connectToMainThread" | "disconnect" | "post" | "on" | "once"
                    )
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "net"
                    && ((class_name == "Socket" && is_net_socket_method_name(&property_name))
                        || (class_name == "Server" && is_net_server_method_name(&property_name)))
                {
                    // `net.Socket` / `net.Server` method reads are callable
                    // values. The call form still lowers through the native
                    // method table; only bare reads use property dispatch.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "sqlite"
                    && class_name == "DatabaseSync"
                    && matches!(
                        property_name.as_str(),
                        "open"
                            | "close"
                            | "exec"
                            | "prepare"
                            | "function"
                            | "aggregate"
                            | "enableDefensive"
                            | "setAuthorizer"
                            | "createTagStore"
                            | "createSession"
                            | "applyChangeset"
                            | "enableLoadExtension"
                            | "loadExtension"
                            | "location"
                    )
                {
                    // `node:sqlite` DatabaseSync methods are callable fields.
                    // Bare reads like `typeof db.close` must not invoke the
                    // lifecycle method as a zero-arg getter.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "sqlite"
                    && class_name == "Session"
                    && matches!(property_name.as_str(), "changeset" | "patchset" | "close")
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "sqlite"
                    && class_name == "SQLTagStore"
                    && matches!(
                        property_name.as_str(),
                        "run" | "get" | "all" | "iterate" | "clear"
                    )
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "sqlite"
                    && class_name == "StatementSync"
                    && matches!(
                        property_name.as_str(),
                        "run"
                            | "get"
                            | "all"
                            | "iterate"
                            | "columns"
                            | "setReadBigInts"
                            | "setReturnArrays"
                            | "setAllowBareNamedParameters"
                            | "setAllowUnknownNamedParameters"
                    )
                {
                    // `node:sqlite` StatementSync methods are callable fields.
                    // Getter properties such as `sourceSQL` and `expandedSQL`
                    // keep the native getter path below.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "Headers" && is_headers_method_name(&property_name) {
                    // A bare Fetch Headers method read (`headers.entries`) is a
                    // function value, not a zero-arg native call. The call form
                    // (`headers.entries()`) is handled by expr_call lowering.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "worker_threads"
                    && class_name == "MessageChannel"
                    && matches!(property_name.as_str(), "port1" | "port2")
                {
                    // #3157: `new MessageChannel()` returns a real heap object
                    // `{ port1, port2 }`. Reading `chan.port1` must be a plain
                    // object field load (returning the port object, whose
                    // methods are closure-valued fields) — NOT a zero-arg
                    // native getter, which would discard the same-process
                    // paired-port delivery. The port objects themselves are not
                    // registered native instances, so `port.postMessage(...)` /
                    // `port.on(...)` already lower as ordinary object-method
                    // calls (invoking the bound closures). parentPort stays on
                    // the native-receiver dispatch path (it's a singleton
                    // handle, not a real object).
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "worker_threads"
                    && class_name == "Worker"
                    && is_worker_instance_value_property(&property_name)
                {
                    // `Worker` exposes data properties (`threadName`,
                    // `resourceLimits`) and method-valued properties (`ref`,
                    // `terminate`, ...). Bare reads must return those object
                    // fields; only call expressions should dispatch through
                    // the native method table.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(
                    module_name.as_str(),
                    "readable_stream"
                        | "writable_stream"
                        | "transform_stream"
                        | "readable_stream_reader"
                        | "writable_stream_writer"
                ) && !matches!(
                    property_name.as_str(),
                    // Getter properties keep the 0-arg NativeMethodCall below
                    // (they really are getters); everything else here is a
                    // callable method.
                    "locked" | "desiredSize" | "closed" | "ready" | "readable" | "writable"
                ) {
                    // #1642: a value-read of a Web Streams *method* (not a
                    // getter) must yield a callable bound-method reference, not
                    // a 0-arg getter call. `lower_member` is reached only for
                    // value-reads (the call form `rs.getReader()` is handled by
                    // `expr_call::lower_call`), so emit a plain `PropertyGet`;
                    // the codegen binds it via `js_class_method_bind` so
                    // `typeof rs.getReader === "function"` and `const f =
                    // rs.getReader; f()` both work.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ClientRequest"
                    && is_http_client_request_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "http"
                    && class_name == "IncomingMessage"
                    && is_http_incoming_message_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "IncomingMessage"
                    && is_http_incoming_message_runtime_property_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ServerResponse"
                    && is_http_server_response_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ServerResponse"
                    && is_http_server_response_runtime_property_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else {
                    // Issue #577 — `req.method` / `res.statusCode` etc.
                    // get rewritten to `__get_<name>` so the property
                    // read dispatches through NATIVE_MODULE_TABLE entries
                    // with class_filter = Some("IncomingMessage" |
                    // "ServerResponse"). Mapping table is the set of
                    // properties exposed via per-class FFI getters in
                    // perry-ext-http-server. Anything not in the set
                    // falls back to the existing bare-method-name
                    // dispatch (covers `request.headers` on fastify
                    // and similar).
                    let property_name = if matches!(module_name.as_str(), "http" | "https") {
                        match (class_name.as_str(), property_name.as_str()) {
                            ("ClientRequest", "method")
                            | ("ClientRequest", "protocol")
                            | ("ClientRequest", "host")
                            | ("ClientRequest", "path")
                            | ("ClientRequest", "aborted")
                            | ("ClientRequest", "connection")
                            | ("ClientRequest", "destroyed")
                            | ("ClientRequest", "finished")
                            | ("ClientRequest", "maxHeadersCount")
                            | ("ClientRequest", "reusedSocket")
                            | ("ClientRequest", "socket")
                            | ("ClientRequest", "writableEnded")
                            | ("ClientRequest", "writableFinished")
                            | ("Agent", "createConnection")
                            | ("Agent", "createSocket")
                            | ("IncomingMessage", "method")
                            | ("IncomingMessage", "url")
                            | ("IncomingMessage", "httpVersion")
                            | ("IncomingMessage", "complete")
                            | ("IncomingMessage", "aborted")
                            | ("IncomingMessage", "destroyed")
                            // Closes #769 followup — client-side `res.statusCode`
                            // (and statusMessage / headers) returned the
                            // 0.0 zero-sentinel from `lower_native_method_call`
                            // because no NativeModSig matched and the receiver
                            // had been pre-tagged ("http", "IncomingMessage"),
                            // so the generic property dispatcher in the runtime
                            // never saw the read. Rewrite to `__get_<prop>` so
                            // the codegen routes through the perry-ext-http
                            // accessor (which knows the client-IncomingMessage
                            // registry).
                            | ("IncomingMessage", "statusCode")
                            | ("IncomingMessage", "statusMessage")
                            | ("IncomingMessage", "headers")
                            | ("ServerResponse", "statusCode")
                            | ("ServerResponse", "headersSent")
                            | ("ServerResponse", "writableEnded")
                            | ("ServerResponse", "writableFinished")
                            // Issue #2210 — `server.headersTimeout` etc.
                            // get rewritten to `__get_<name>` so the read
                            // dispatches through the per-prop FFI in
                            // perry-ext-http-server (Phase 1 returns the
                            // stored numeric default; Phase 2 will reflect
                            // the live hyper accept-loop state).
                            | ("HttpServer", "headersTimeout")
                            | ("HttpServer", "keepAliveTimeout")
                            | ("HttpServer", "keepAliveTimeoutBuffer")
                            | ("HttpServer", "requestTimeout")
                            | ("HttpServer", "timeout")
                            | ("HttpServer", "maxHeadersCount")
                            | ("HttpServer", "maxRequestsPerSocket")
                            | ("HttpsServer", "headersTimeout")
                            | ("HttpsServer", "keepAliveTimeout")
                            | ("HttpsServer", "keepAliveTimeoutBuffer")
                            | ("HttpsServer", "requestTimeout")
                            | ("HttpsServer", "timeout")
                            | ("HttpsServer", "maxHeadersCount")
                            | ("HttpsServer", "maxRequestsPerSocket") => {
                                format!("__get_{}", property_name)
                            }
                            _ => property_name,
                        }
                    } else {
                        property_name
                    };
                    let class_filter =
                        if matches!(module_name.as_str(), "http" | "https" | "events") {
                            Some(class_name.clone())
                        } else {
                            None
                        };
                    // For properties that map to FFI functions, generate a NativeMethodCall
                    // with no args (property getter)
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::NativeMethodCall {
                        module: module_name,
                        class_name: class_filter,
                        object: Some(Box::new(object_expr)),
                        method: property_name,
                        args: Vec::new(),
                    });
                }
            }
        }
    }

    // Inline `new TextDecoder(...).encoding | .fatal | .ignoreBOM`.
    if let ast::Expr::New(new_expr) = member.obj.as_ref() {
        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
            if class_ident.sym.as_ref() == "TextDecoder" {
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let prop_name = prop_ident.sym.as_ref();
                    if matches!(prop_name, "encoding" | "fatal" | "ignoreBOM") {
                        let d =
                            super::expr_new::lower_text_decoder_new(ctx, new_expr.args.as_deref())?;
                        return Ok(match prop_name {
                            "encoding" => Expr::TextDecoderEncoding(Box::new(d)),
                            "fatal" => Expr::TextDecoderFatal(Box::new(d)),
                            _ => Expr::TextDecoderIgnoreBom(Box::new(d)),
                        });
                    }
                }
            }
        }
    }

    // TextEncoder / TextDecoder property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            let prop_name = prop_ident.sym.as_ref();
            let is_text_encoder = ctx
                .lookup_local_type(&obj_name)
                .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                .unwrap_or(false);
            let is_text_decoder = ctx
                .lookup_local_type(&obj_name)
                .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                .unwrap_or(false);
            if is_text_encoder && prop_name == "encoding" {
                return Ok(Expr::String("utf-8".to_string()));
            }
            if is_text_decoder {
                match prop_name {
                    "encoding" => {
                        let d = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::TextDecoderEncoding(Box::new(d)));
                    }
                    "fatal" => {
                        let d = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::TextDecoderFatal(Box::new(d)));
                    }
                    "ignoreBOM" => {
                        let d = lower_expr(ctx, &member.obj)?;
                        return Ok(Expr::TextDecoderIgnoreBom(Box::new(d)));
                    }
                    _ => {}
                }
            }
        }
    }

    // RegExp property access: regex.source / .flags / .lastIndex
    // Detect when receiver is a regex literal or local typed as RegExp.
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        let prop_name = prop_ident.sym.as_ref();
        if prop_name == "source" || prop_name == "flags" || prop_name == "lastIndex" {
            let is_regex_obj = match member.obj.as_ref() {
                ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                ast::Expr::Ident(ident) => ctx
                    .lookup_local_type(ident.sym.as_ref())
                    .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                    .unwrap_or(false),
                _ => false,
            };
            if is_regex_obj {
                let regex_expr = lower_expr(ctx, &member.obj)?;
                if matches!(&regex_expr, Expr::RegExp { .. })
                    || matches!(&regex_expr, Expr::LocalGet(_))
                {
                    return Ok(match prop_name {
                        "source" => Expr::RegExpSource(Box::new(regex_expr)),
                        "flags" => Expr::RegExpFlags(Box::new(regex_expr)),
                        "lastIndex" => Expr::RegExpLastIndex(Box::new(regex_expr)),
                        _ => unreachable!(),
                    });
                }
            }
        }
        // RegExpExecArray.index / .groups — receiver is a local that holds the result
        // of regex.exec(...). The runtime stores the most recent exec metadata in
        // thread-locals which RegExpExecIndex/Groups read.
        if prop_name == "index" || prop_name == "groups" {
            // Strip non-null assertion (m1! → m1)
            let inner = match member.obj.as_ref() {
                ast::Expr::TsNonNull(nn) => nn.expr.as_ref(),
                other => other,
            };
            if let ast::Expr::Ident(ident) = inner {
                if ctx.regex_exec_locals.contains(&ident.sym.to_string()) {
                    return Ok(if prop_name == "index" {
                        Expr::RegExpExecIndex
                    } else {
                        Expr::RegExpExecGroups
                    });
                }
            }
        }
    }

    // Tagged-template `.raw` — recognize `<strings>.raw` where the
    // receiver is an Array-typed local (the typical signature is
    // `function tag(strings: TemplateStringsArray, ...)`, which Perry's
    // HIR types as a plain `Type::Array(Type::String)` after stripping
    // the alias). Folds to `Expr::TemplateRaw`, which the codegen
    // resolves to `js_template_raw(arr)` — a thread-local lookup of the
    // raw-strings array registered by the matching
    // `Expr::TaggedTemplateStrings` build at the call site.
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if prop_ident.sym.as_ref() == "raw" {
            if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                let recv_ty = ctx.lookup_local_type(ident.sym.as_ref());
                let is_array = match recv_ty {
                    Some(perry_types::Type::Array(_)) | Some(perry_types::Type::Tuple(_)) => true,
                    Some(perry_types::Type::Named(n)) if n == "TemplateStringsArray" => true,
                    _ => false,
                };
                if is_array {
                    let arr_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::TemplateRaw(Box::new(arr_expr)));
                }
            }
        }
    }

    let mut object_expr = lower_expr(ctx, &member.obj)?;
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        if let Some(value) = ws_ready_state_value(prop_ident.sym.as_ref()) {
            if is_ws_ready_state_receiver(ctx, member.obj.as_ref(), &object_expr) {
                return Ok(Expr::Number(value));
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
                    let outer_is_prototype_or_proto = matches!(
                        &member.prop,
                        ast::MemberProp::Ident(p) if p.sym.as_ref() == "prototype"
                            || p.sym.as_ref() == "__proto__"
                    );
                    let receiver_is_namespace_value = matches!(
                        property.as_str(),
                        "Atomics" | "crypto" | "WebAssembly" | "localStorage" | "sessionStorage"
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
                    if !outer_is_prototype_or_proto
                        && !receiver_is_namespace_value
                        && !outer_is_websocket_static
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
                    Expr::PropertyGet { object, property } => {
                        matches!(object.as_ref(), Expr::GlobalGet(0)) && property.as_str() == name
                    }
                    _ => false,
                };
                if is_global_builtin {
                    if let Some(len) = crate::analysis::builtin_constructor_length(name) {
                        return Ok(Expr::Number(len as f64));
                    }
                }
            }
            if let Expr::PropertyGet {
                object: inner,
                property,
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
        let allow_unimplemented = std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
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
        if !allow_unimplemented
            && !obj_is_named_import
            && perry_api_manifest::module_has_any_entries(module)
            && perry_api_manifest::module_has_symbol(module, prop).is_none()
        {
            // #925: when there's a known supported equivalent for this
            // shape, append it to the error so the user doesn't have to
            // grep through the manifest to find the replacement.
            let hint = super::unimpl_hints::module_member_hint(module, prop)
                .map(|h| format!(" {h}"))
                .unwrap_or_default();
            let msg = format!(
                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                module, prop, hint,
            );
            // #2309: defer when tree-shaking (sink armed for this node_modules
            // module); re-raised only if the module survives pruning.
            if !crate::try_defer_refusal(msg.clone(), member.span.lo.0) {
                // #3896: a bare *value read* of an absent member on a Node
                // builtin module namespace/default object is an ordinary
                // property miss → `undefined` (e.g. `dns/promises.ADDRCONFIG`,
                // which Node also doesn't export but reads as undefined). Calls
                // (`ns.foo()`) keep rejecting — `lower_call` set the callee
                // marker, so `member_is_call_callee` is true there. Only Node
                // core modules relax; unenumerated npm packages keep the strict
                // gate (and the tree-shaking defer above).
                if !member_is_call_callee && perry_api_manifest::is_node_core_module(module) {
                    return Ok(Expr::Undefined);
                }
                crate::lower_bail!(member.span, "{}", msg);
            }
        }
    }

    match &member.prop {
        ast::MemberProp::Ident(ident) => {
            let property = ident.sym.to_string();
            Ok(Expr::PropertyGet { object, property })
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
            //   - the refusal pass is enabled (`PERRY_ALLOW_DYNAMIC_STDLIB=0` /
            //     `perry.allowDynamicStdlibDispatch: false`; on by default),
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
                    if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
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
                        object,
                        property: key.clone(),
                    });
                }
            }
            Ok(Expr::IndexGet { object, index })
        }
        ast::MemberProp::PrivateName(private) => {
            // Private field access: this.#field -> PropertyGet with "#field"
            let property = format!("#{}", private.name);
            Ok(Expr::PropertyGet { object, property })
        }
    }
}

/// #503 — Node-core stdlib namespace receivers whose dynamic (`obj[x]`)
/// member access is refused at compile time. These are the namespaces
/// the issue calls out: the well-known shapes used by string-based
/// obfuscation in malicious npm packages. Globals (`process`, `Buffer`)
/// and `require`-imported core modules are both covered — Buffer is
/// intentionally omitted because it is a class constructor (`new Buffer`)
/// rather than a namespace; the meaningful attack surface there is the
/// constructor itself, not dynamic property access. Keep this list in
/// sync with the docs in `docs/src/security/dynamic-dispatch.md`.
const STDLIB_NAMESPACE_NAMES: &[&str] = &[
    "process",
    "fs",
    "crypto",
    "child_process",
    "dgram",
    "net",
    "os",
    "path",
    "http",
    "https",
    "http2",
    "stream",
    "url",
    "util",
    "events",
    "dns",
    "tls",
    "querystring",
    "zlib",
    "async_hooks",
    "readline",
    "string_decoder",
    "test",
    "tty",
    "worker_threads",
];

/// #1723 — is `member` the auditable `ns[dynamicKey].staticMember` shape, where
/// the dynamic index merely selects a stdlib SUB-namespace (e.g. `path.win32` /
/// `path.posix`) and the member actually used is a *source-visible* static name?
///
/// This is the legit counterpart of the #503 obfuscation pattern
/// `ns[runtimeVar]()` — which HIDES the called method behind a runtime string.
/// Here the method name is in plaintext (`.matchesGlob`, or a literal-string
/// key that folds to a static property), and the dynamic index only picks among
/// a namespace's tiny, known set of sub-namespaces, every one of which exposes
/// the same API surface the static member already names. So nothing is hidden,
/// and the #503 refusal should not fire on the nested `ns[dynamicKey]`. The
/// discriminator is the *enclosing access shape*, not the binding origin, so
/// `require()`, `import * as`, and default-import forms all behave identically.
///
/// Returns true only when:
///   - `member.prop` is static — an `Ident` or a computed STRING-LITERAL key
///     (a numeric/dynamic key would not be auditable), AND
///   - `member.obj` (transparent TS/paren wrappers peeled) is `recv[<nonLiteral>]`
///     where `recv` resolves to a stdlib namespace.
///
/// `ns[d1][d2]` (chained dynamic — the enclosing prop is a non-literal computed
/// key) is NOT matched and stays refused. Surfaced by the #800 node-core radar:
/// `test-path-glob.js` does `path[platform].matchesGlob(path, glob)`.
pub(super) fn stdlib_ns_subnamespace_static_access(
    ctx: &super::LoweringContext,
    member: &ast::MemberExpr,
) -> bool {
    // Enclosing access must name a STATIC (auditable) property.
    let prop_is_static = match &member.prop {
        ast::MemberProp::Ident(_) => true,
        ast::MemberProp::Computed(c) => matches!(*c.expr, ast::Expr::Lit(ast::Lit::Str(_))),
        _ => false,
    };
    if !prop_is_static {
        return false;
    }
    // Object must be `<stdlib-ns>[<nonLiteralKey>]`.
    let mut obj = member.obj.as_ref();
    loop {
        match obj {
            ast::Expr::Paren(p) => obj = p.expr.as_ref(),
            ast::Expr::TsAs(a) => obj = a.expr.as_ref(),
            ast::Expr::TsNonNull(a) => obj = a.expr.as_ref(),
            ast::Expr::TsTypeAssertion(a) => obj = a.expr.as_ref(),
            ast::Expr::TsConstAssertion(a) => obj = a.expr.as_ref(),
            ast::Expr::TsSatisfies(a) => obj = a.expr.as_ref(),
            _ => break,
        }
    }
    let inner = match obj {
        ast::Expr::Member(m) => m,
        _ => return false,
    };
    let inner_is_dynamic = match &inner.prop {
        ast::MemberProp::Computed(c) => !matches!(*c.expr, ast::Expr::Lit(ast::Lit::Str(_))),
        _ => false,
    };
    if !inner_is_dynamic {
        return false;
    }
    stdlib_namespace_receiver(ctx, inner.obj.as_ref()).is_some()
}

/// #503 — does the given AST receiver expression resolve to a known
/// stdlib namespace? Recognised shapes:
///   - bare ident matching one of `STDLIB_NAMESPACE_NAMES` (global
///     `process` or top-level imported `fs` etc.),
///   - bare ident bound to a stdlib alias via `import x from 'fs'`
///     (`ctx.builtin_module_aliases` populated by `require()` and ESM
///     default imports), or
///   - bare ident bound to a namespace import (`import * as fs from
///     'fs'`) via `ctx.native_modules` with a `None` method-name.
///
/// Returns the canonical stdlib namespace name (e.g. `"fs"`) when a
/// match is found, so the diagnostic can name the namespace concretely.
pub(super) fn stdlib_namespace_receiver(
    ctx: &super::LoweringContext,
    obj: &ast::Expr,
) -> Option<&'static str> {
    // TS type-position wrappers like `(process as any)` and
    // `<any>process` parse as `TsAsExpr` / `TsTypeAssertion`, and the
    // `(...)` itself shows up as a `Paren`. Strip them so an idiomatic
    // `(process as any)[k]()` still surfaces `process` as the receiver.
    let mut current = obj;
    loop {
        match current {
            ast::Expr::Paren(p) => current = p.expr.as_ref(),
            ast::Expr::TsAs(a) => current = a.expr.as_ref(),
            ast::Expr::TsTypeAssertion(a) => current = a.expr.as_ref(),
            ast::Expr::TsNonNull(a) => current = a.expr.as_ref(),
            ast::Expr::TsConstAssertion(a) => current = a.expr.as_ref(),
            ast::Expr::TsSatisfies(a) => current = a.expr.as_ref(),
            _ => break,
        }
    }
    let ident = match current {
        ast::Expr::Ident(ident) => ident,
        _ => return None,
    };
    let name = ident.sym.as_ref();

    // #1701: a LOCAL binding (function param / `let` / `const`) that merely
    // shares a name with a stdlib namespace is NOT the namespace — it shadows
    // it. hono's trie-router has `path` (a URL-path string param) and does
    // `path[0] === "/"`; treating that local as the `node:path` namespace
    // false-fired the #503 refusal and blocked the whole package from
    // compiling. A real stdlib namespace is never a local: it's the global
    // (`process`) or an import, which the alias / namespace-import branches
    // below resolve. So skip the direct name-match when `name` is shadowed by
    // a local. (If a package shadows `process` with its own local, that local
    // is genuinely theirs and likewise shouldn't be refused.)
    if ctx.lookup_local(name).is_some() {
        return None;
    }

    // Direct global / module specifier match.
    if let Some(canon) = STDLIB_NAMESPACE_NAMES.iter().find(|n| **n == name) {
        return Some(*canon);
    }

    // `require()` / default-import alias: `import fs from 'fs'` →
    // builtin_module_aliases["fs"] = "fs", but the user may rename:
    // `import myFs from 'fs'` → ["myFs"] = "fs". Resolve to the
    // canonical specifier.
    for (local, module) in ctx.builtin_module_aliases.iter() {
        if local == name {
            if let Some(canon) = STDLIB_NAMESPACE_NAMES
                .iter()
                .find(|n| **n == module.as_str())
            {
                return Some(*canon);
            }
        }
    }

    // Namespace import: `import * as fs from 'fs'` — tracked as a
    // native_modules entry with method_name = None.
    for (local, module, method) in ctx.native_modules.iter() {
        if local == name && method.is_none() {
            if let Some(canon) = STDLIB_NAMESPACE_NAMES
                .iter()
                .find(|n| **n == module.as_str())
            {
                return Some(*canon);
            }
        }
    }

    None
}

/// Issue #562 — does `prop` name a stream-API method or property on the
/// given stream module? Used to gate the native-instance property
/// rerouting so subclass-declared fields fall through to regular object
/// property access. Mirrors the methods + accessors hardcoded in
/// `crates/perry-codegen/src/lower_call.rs`'s
/// `module == "<stream_kind>"` arms.
fn is_stream_api_member(module: &str, prop: &str) -> bool {
    match module {
        "readable_stream" => matches!(
            prop,
            "getReader"
                | "cancel"
                | "tee"
                | "pipeTo"
                | "pipeThrough"
                | "locked"
                | "enqueue"
                | "close"
                | "error"
                | "desiredSize"
        ),
        "readable_stream_reader" => {
            matches!(prop, "read" | "releaseLock" | "cancel" | "closed")
        }
        "writable_stream" => matches!(prop, "getWriter" | "abort" | "close" | "locked"),
        "writable_stream_writer" => matches!(
            prop,
            "write" | "close" | "abort" | "releaseLock" | "closed" | "ready" | "desiredSize"
        ),
        "transform_stream" => matches!(prop, "readable" | "writable"),
        _ => false,
    }
}

fn is_classic_stream_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "read"
            | "push"
            | "pipe"
            | "unpipe"
            | "pause"
            | "resume"
            | "destroy"
            | "setEncoding"
            | "isPaused"
            | "write"
            | "end"
            | "cork"
            | "uncork"
            | "setDefaultEncoding"
            | "compose"
            | "on"
            | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "listeners"
            | "rawListeners"
            | "eventNames"
            | "listenerCount"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "setMaxListeners"
            | "getMaxListeners"
    )
}

fn is_http_incoming_message_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "addListener"
            | "setEncoding"
            | "setTimeout"
            | "pause"
            | "resume"
            | "destroy"
            | "read"
    )
}

fn is_http_client_request_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "end"
            | "write"
            | "setHeader"
            | "setTimeout"
            | "listenerCount"
            | "getHeader"
            | "hasHeader"
            | "removeHeader"
            | "getHeaderNames"
            | "getHeaders"
            | "getRawHeaderNames"
            | "abort"
            | "destroy"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setNoDelay"
            | "setSocketKeepAlive"
    )
}

fn is_http_incoming_message_runtime_property_name(prop: &str) -> bool {
    matches!(
        prop,
        "method"
            | "url"
            | "httpVersion"
            | "headers"
            | "rawHeaders"
            | "headersDistinct"
            | "trailers"
            | "rawTrailers"
            | "trailersDistinct"
            | "complete"
            | "aborted"
            | "destroyed"
            | "socket"
            | "connection"
            | "signal"
            | "remoteAddress"
            | "remotePort"
    )
}

fn is_http_server_response_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "setHeader"
            | "getHeader"
            | "removeHeader"
            | "hasHeader"
            | "getHeaders"
            | "getHeaderNames"
            | "appendHeader"
            | "setHeaders"
            | "writeHead"
            | "write"
            | "addTrailers"
            | "end"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setTimeout"
            | "writeEarlyHints"
            | "writeContinue"
            | "writeProcessing"
            | "on"
            | "addListener"
    )
}

fn is_http_server_response_runtime_property_name(prop: &str) -> bool {
    matches!(
        prop,
        "statusCode"
            | "statusMessage"
            | "headersSent"
            | "writableEnded"
            | "writableFinished"
            | "finished"
            | "sendDate"
            | "strictContentLength"
            | "req"
            | "socket"
            | "connection"
    )
}

fn is_dns_resolver_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "cancel"
            | "getServers"
            | "setServers"
            | "setLocalAddress"
            | "resolve"
            | "resolve4"
            | "resolve6"
            | "resolveAny"
            | "resolveCaa"
            | "resolveCname"
            | "resolveMx"
            | "resolveNaptr"
            | "resolveNs"
            | "resolvePtr"
            | "resolveSoa"
            | "resolveSrv"
            | "resolveTlsa"
            | "resolveTxt"
            | "reverse"
    )
}

fn is_console_instance_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "log"
            | "info"
            | "debug"
            | "dir"
            | "dirxml"
            | "error"
            | "warn"
            | "count"
            | "countReset"
            | "group"
            | "groupCollapsed"
            | "groupEnd"
            | "clear"
            | "profile"
            | "profileEnd"
            | "timeStamp"
    )
}

fn is_dgram_socket_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "send"
            | "bind"
            | "close"
            | "address"
            | "connect"
            | "disconnect"
            | "addMembership"
            | "dropMembership"
            | "setBroadcast"
            | "setMulticastTTL"
            | "setMulticastLoopback"
            | "setMulticastInterface"
            | "setTTL"
            | "setRecvBufferSize"
            | "setSendBufferSize"
            | "getRecvBufferSize"
            | "getSendBufferSize"
            | "ref"
            | "unref"
    )
}

fn is_net_socket_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "address"
            | "connect"
            | "destroy"
            | "destroySoon"
            | "end"
            | "pause"
            | "ref"
            | "resetAndDestroy"
            | "resume"
            | "setEncoding"
            | "setKeepAlive"
            | "setNoDelay"
            | "setTimeout"
            | "unref"
            | "write"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
            | "upgradeToTLS"
            | "setDefaultEncoding"
            | "cork"
            | "uncork"
    )
}

fn is_net_server_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "address"
            | "close"
            | "getConnections"
            | "listen"
            | "ref"
            | "unref"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
    )
}

fn is_headers_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "append"
            | "delete"
            | "entries"
            | "forEach"
            | "get"
            | "getSetCookie"
            | "has"
            | "keys"
            | "set"
            | "values"
    )
}

fn is_url_pattern_data_property(prop: &str) -> bool {
    matches!(
        prop,
        "protocol"
            | "username"
            | "password"
            | "hostname"
            | "port"
            | "pathname"
            | "search"
            | "hash"
            | "hasRegExpGroups"
    )
}

fn is_worker_instance_value_property(prop: &str) -> bool {
    matches!(
        prop,
        "threadId"
            | "threadName"
            | "resourceLimits"
            | "stdin"
            | "stdout"
            | "stderr"
            | "performance"
            | "getHeapStatistics"
            | "cpuUsage"
            | "getHeapSnapshot"
            | "startCpuProfile"
            | "startHeapProfile"
            | "postMessage"
            | "terminate"
            | "ref"
            | "unref"
            | "on"
            | "once"
            | "off"
    )
}

/// #1378: `process.features` literal. Boolean capability flags Node
/// exposes so libraries can detect what the runtime links in. Perry
/// links its own networking/TLS stack; the values here reflect what
/// the runtime *actually* supports, not what Node would say — readers
/// generally branch on `openssl_is_boringssl` / `quic` / `typescript`
/// rather than rejecting any unrecognised value, so a Perry-honest
/// shape is safer than parroting Node's.
/// `process.allowedNodeEnvironmentFlags` (#2589) — the Set of flags Node
/// accepts from `NODE_OPTIONS` / the V8 environment. Perry binaries are
/// AOT and don't honour `NODE_OPTIONS`-style runtime flags, but consumers
/// feature-detect on this being a real, non-empty `Set` (e.g.
/// `flags instanceof Set`, `flags.size > 0`, `flags.has("--no-warnings")`,
/// iteration). We materialise it as a `Set` populated with a
/// Node-compatible flag list (via `Expr::SetNewFromArray`) so the
/// observable shape matches. The exact membership varies by Node build;
/// this list mirrors a recent Node and is intentionally not asserted
/// byte-for-byte by parity tests.
fn process_allowed_node_flags_literal() -> Expr {
    const FLAGS: &[&str] = &[
        "--abort-on-uncaught-exception",
        "--addons",
        "--allow-addons",
        "--allow-child-process",
        "--allow-fs-read",
        "--allow-fs-write",
        "--allow-inspector",
        "--allow-net",
        "--allow-wasi",
        "--allow-worker",
        "--async-context-frame",
        "--conditions",
        "--cpu-prof",
        "--cpu-prof-dir",
        "--cpu-prof-interval",
        "--cpu-prof-name",
        "--debug-arraybuffer-allocations",
        "--debug-port",
        "--deprecation",
        "--diagnostic-dir",
        "--disable-proto",
        "--disable-sigusr1",
        "--disable-warning",
        "--disable-wasm-trap-handler",
        "--disallow-code-generation-from-strings",
        "--dns-result-order",
        "--enable-etw-stack-walking",
        "--enable-fips",
        "--enable-network-family-autoselection",
        "--enable-source-maps",
        "--entry-url",
        "--es-module-specifier-resolution",
        "--experimental-abortcontroller",
        "--experimental-addon-modules",
        "--experimental-detect-module",
        "--experimental-eventsource",
        "--experimental-fetch",
        "--experimental-global-customevent",
        "--experimental-global-navigator",
        "--experimental-global-webcrypto",
        "--experimental-import-meta-resolve",
        "--experimental-json-modules",
        "--experimental-loader",
        "--experimental-modules",
        "--experimental-print-required-tla",
        "--experimental-quic",
        "--experimental-repl-await",
        "--experimental-report",
        "--experimental-require-module",
        "--experimental-shadow-realm",
        "--experimental-specifier-resolution",
        "--experimental-sqlite",
        "--experimental-strip-types",
        "--experimental-test-isolation",
        "--experimental-top-level-await",
        "--experimental-transform-types",
        "--experimental-vm-modules",
        "--experimental-wasi-unstable-preview1",
        "--experimental-wasm-modules",
        "--experimental-websocket",
        "--experimental-webstorage",
        "--experimental-worker",
        "--expose-gc",
        "--extra-info-on-fatal-exception",
        "--force-async-hooks-checks",
        "--force-context-aware",
        "--force-fips",
        "--force-node-api-uncaught-exceptions-policy",
        "--frozen-intrinsics",
        "--global-search-paths",
        "--heap-prof",
        "--heap-prof-dir",
        "--heap-prof-interval",
        "--heap-prof-name",
        "--heapsnapshot-near-heap-limit",
        "--heapsnapshot-signal",
        "--http-parser",
        "--icu-data-dir",
        "--import",
        "--input-type",
        "--insecure-http-parser",
        "--inspect",
        "--inspect-brk",
        "--inspect-port",
        "--inspect-publish-uid",
        "--inspect-wait",
        "--interpreted-frames-native-stack",
        "--jitless",
        "--loader",
        "--localstorage-file",
        "--max-http-header-size",
        "--max-old-space-size",
        "--max-old-space-size-percentage",
        "--max-semi-space-size",
        "--napi-modules",
        "--network-family-autoselection",
        "--network-family-autoselection-attempt-timeout",
        "--no-addons",
        "--no-allow-addons",
        "--no-allow-child-process",
        "--no-allow-inspector",
        "--no-allow-net",
        "--no-allow-wasi",
        "--no-allow-worker",
        "--no-async-context-frame",
        "--no-cpu-prof",
        "--no-debug-arraybuffer-allocations",
        "--no-deprecation",
        "--no-disable-sigusr1",
        "--no-disable-wasm-trap-handler",
        "--no-enable-fips",
        "--no-enable-source-maps",
        "--no-entry-url",
        "--no-experimental-addon-modules",
        "--no-experimental-detect-module",
        "--no-experimental-eventsource",
        "--no-experimental-global-navigator",
        "--no-experimental-import-meta-resolve",
        "--no-experimental-print-required-tla",
        "--no-experimental-repl-await",
        "--no-experimental-require-module",
        "--no-experimental-shadow-realm",
        "--no-experimental-sqlite",
        "--no-experimental-transform-types",
        "--no-experimental-vm-modules",
        "--no-experimental-websocket",
        "--no-experimental-webstorage",
        "--no-extra-info-on-fatal-exception",
        "--no-force-async-hooks-checks",
        "--no-force-context-aware",
        "--no-force-fips",
        "--no-force-node-api-uncaught-exceptions-policy",
        "--no-frozen-intrinsics",
        "--no-global-search-paths",
        "--no-heap-prof",
        "--no-insecure-http-parser",
        "--no-inspect",
        "--no-inspect-brk",
        "--no-inspect-wait",
        "--no-network-family-autoselection",
        "--no-node-snapshot",
        "--no-openssl-legacy-provider",
        "--no-openssl-shared-config",
        "--no-pending-deprecation",
        "--no-permission",
        "--no-permission-audit",
        "--no-preserve-symlinks",
        "--no-preserve-symlinks-main",
        "--no-report-compact",
        "--no-report-exclude-env",
        "--no-report-exclude-network",
        "--no-report-on-fatalerror",
        "--no-report-on-signal",
        "--no-report-uncaught-exception",
        "--no-require-module",
        "--no-strip-types",
        "--no-test-only",
        "--no-throw-deprecation",
        "--no-tls-max-v1.2",
        "--no-tls-max-v1.3",
        "--no-tls-min-v1.0",
        "--no-tls-min-v1.1",
        "--no-tls-min-v1.2",
        "--no-tls-min-v1.3",
        "--no-trace-deprecation",
        "--no-trace-env",
        "--no-trace-env-js-stack",
        "--no-trace-env-native-stack",
        "--no-trace-exit",
        "--no-trace-promises",
        "--no-trace-sigint",
        "--no-trace-sync-io",
        "--no-trace-tls",
        "--no-trace-uncaught",
        "--no-trace-warnings",
        "--no-track-heap-objects",
        "--no-use-bundled-ca",
        "--no-use-env-proxy",
        "--no-use-openssl-ca",
        "--no-use-system-ca",
        "--no-verify-base-objects",
        "--no-warnings",
        "--no-watch",
        "--no-watch-preserve-output",
        "--no-zero-fill-buffers",
        "--node-memory-debug",
        "--node-snapshot",
        "--openssl-config",
        "--openssl-legacy-provider",
        "--openssl-shared-config",
        "--pending-deprecation",
        "--perf-basic-prof",
        "--perf-basic-prof-only-functions",
        "--perf-prof",
        "--perf-prof-unwinding-info",
        "--permission",
        "--permission-audit",
        "--preserve-symlinks",
        "--preserve-symlinks-main",
        "--prof-process",
        "--redirect-warnings",
        "--report-compact",
        "--report-dir",
        "--report-directory",
        "--report-exclude-env",
        "--report-exclude-network",
        "--report-filename",
        "--report-on-fatalerror",
        "--report-on-signal",
        "--report-signal",
        "--report-uncaught-exception",
        "--require",
        "--require-module",
        "--secure-heap",
        "--secure-heap-min",
        "--snapshot-blob",
        "--stack-trace-limit",
        "--strip-types",
        "--test-coverage-branches",
        "--test-coverage-exclude",
        "--test-coverage-functions",
        "--test-coverage-include",
        "--test-coverage-lines",
        "--test-global-setup",
        "--test-isolation",
        "--test-name-pattern",
        "--test-only",
        "--test-reporter",
        "--test-reporter-destination",
        "--test-rerun-failures",
        "--test-shard",
        "--test-skip-pattern",
        "--throw-deprecation",
        "--title",
        "--tls-cipher-list",
        "--tls-keylog",
        "--tls-max-v1.2",
        "--tls-max-v1.3",
        "--tls-min-v1.0",
        "--tls-min-v1.1",
        "--tls-min-v1.2",
        "--tls-min-v1.3",
        "--trace-deprecation",
        "--trace-env",
        "--trace-env-js-stack",
        "--trace-env-native-stack",
        "--trace-event-categories",
        "--trace-event-file-pattern",
        "--trace-events-enabled",
        "--trace-exit",
        "--trace-promises",
        "--trace-require-module",
        "--trace-sigint",
        "--trace-sync-io",
        "--trace-tls",
        "--trace-uncaught",
        "--trace-warnings",
        "--track-heap-objects",
        "--unhandled-rejections",
        "--use-bundled-ca",
        "--use-env-proxy",
        "--use-largepages",
        "--use-openssl-ca",
        "--use-system-ca",
        "--v8-pool-size",
        "--verify-base-objects",
        "--warnings",
        "--watch",
        "--watch-kill-signal",
        "--watch-path",
        "--watch-preserve-output",
        "--webstorage",
        "--zero-fill-buffers",
        "-C",
        "-r",
    ];
    Expr::SetNewFromArray(Box::new(Expr::Array(
        FLAGS
            .iter()
            .map(|f| Expr::String((*f).to_string()))
            .collect(),
    )))
}

fn process_features_literal() -> Expr {
    fn b(k: &str, v: bool) -> (String, Expr) {
        (k.to_string(), Expr::Bool(v))
    }
    Expr::Object(vec![
        b("inspector", false),
        b("debug", false),
        b("uv", false),
        b("ipv6", true),
        b("tls_alpn", true),
        b("tls_sni", true),
        b("tls_ocsp", true),
        b("tls", true),
        b("openssl_is_boringssl", false),
        b("cached_builtins", false),
        b("require_module", false),
        b("quic", false),
        // Perry compiles TypeScript natively (AOT) — surface as
        // `"transform"` to distinguish from Node's `"strip"` mode.
        (
            "typescript".to_string(),
            Expr::String("transform".to_string()),
        ),
    ])
}
