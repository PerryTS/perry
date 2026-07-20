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
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};

// Tier 2.3 split (chore/split-large-files): cohesive groups moved into sibling
// modules under `expr_member/`. Pure code move — re-export every moved item
// referenced from this trunk (and across siblings) so existing call paths keep
// resolving via `super::*` in each sibling.
mod member_tail;
mod native_dispatch;
mod private_guard;
mod process_literals;
mod process_props;
mod stdlib_guard;

pub(crate) use member_tail::lower_member_tail;
pub(crate) use native_dispatch::{
    is_blob_getter_name, is_classic_stream_getter_name, is_classic_stream_method_name,
    is_console_instance_method_name, is_dgram_socket_method_name, is_dns_resolver_method_name,
    is_fetch_response_getter_name, is_headers_method_name, is_http_client_request_method_name,
    is_http_incoming_message_method_name, is_http_incoming_message_runtime_property_name,
    is_http_server_response_method_name, is_http_server_response_runtime_property_name,
    is_native_dispatch_member, is_net_server_method_name, is_net_socket_method_name,
    is_stream_api_member, is_url_pattern_data_property, is_worker_instance_value_property,
};
pub(crate) use private_guard::{wrap_private_guard, PRIV_OP_READ, PRIV_OP_WRITE};
pub(crate) use process_literals::{process_allowed_node_flags_literal, process_features_literal};
pub(crate) use process_props::{
    is_ws_ready_state_receiver, lower_process_named_property, process_metadata_native_property,
    process_native_property, ws_ready_state_value,
};
pub(crate) use stdlib_guard::{stdlib_namespace_receiver, stdlib_ns_subnamespace_static_access};

/// #5009: resolve a build-time `perry.define` of `process.env.<name>` to the
/// HIR literal it should fold to, if one is configured for this build. Returns
/// `None` when there is no define for `name` (the caller then emits the normal
/// runtime `EnvGet`).
pub(crate) fn env_define_literal(name: &str) -> Option<Expr> {
    crate::ir::env_define_lookup(name).map(|d| match d {
        crate::ir::EnvDefine::Str(s) => Expr::String(s),
        crate::ir::EnvDefine::Bool(b) => Expr::Bool(b),
        crate::ir::EnvDefine::Num(n) => Expr::Number(n),
        crate::ir::EnvDefine::Null => Expr::Null,
    })
}

/// Peel transparent TS/paren wrappers (`as`, `!`, `satisfies`, `<T>x`, `(x)`)
/// off an expression. Promoted from a nested fn inside `lower_member_inner` so
/// both the early checks (in this trunk) and the moved tail
/// (`member_tail::lower_member_tail`) can share it. Pure code move.
pub(crate) fn unwrap_transparent(e: &ast::Expr) -> &ast::Expr {
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

/// The property name of a member access as a source-visible STATIC string, for
/// EITHER a dot access (`obj.name`) or a bracket access with a string-literal
/// key (`obj[\"name\"]`). Per spec these are equivalent for string keys, so the
/// builtin constant-fold / reified-static-value paths that key off a dot
/// property name must fire for the string-literal computed form too — otherwise
/// `Number[\"POSITIVE_INFINITY\"]` / `Math[\"E\"]` / `Array[\"prototype\"]` fall
/// through the static resolution and collapse to a bare `globalThis.<key>` read
/// (undefined). Returns `None` for a dynamic (non-string-literal) computed key,
/// which must keep the runtime index path. (Test262 property-accessors.)
pub(crate) fn static_member_prop_name(prop: &ast::MemberProp) -> Option<String> {
    match prop {
        ast::MemberProp::Ident(p) => Some(p.sym.to_string()),
        ast::MemberProp::Computed(c) => match c.expr.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().map(|v| v.to_string()),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

/// The well-known-symbol member names exposed on the `Symbol` constructor
/// (`Symbol.iterator`, `Symbol.asyncIterator`, `Symbol.toPrimitive`, …). Both the
/// dot-access fold and the computed-access fold (#6676, `Symbol["iterator"]`)
/// rewrite these to the `@@__perry_wk_<name>` sentinel that `js_symbol_for`
/// resolves from the well-known cache, so a single list keeps the two forms in
/// lockstep. `dispose`/`asyncDispose` are included because Perry surfaces them as
/// well-known symbols (`using`/`await using`).
pub(crate) fn is_well_known_symbol_member(name: &str) -> bool {
    matches!(
        name,
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
    )
}

/// Fold a well-known-symbol member access on the `Symbol` constructor to the
/// `@@__perry_wk_<name>` sentinel that `js_symbol_for` resolves from the
/// well-known cache. This is the ONE resolver shared by every access form that
/// reaches `Symbol.<well-known>`, so they all yield the identity-equal cached
/// symbol:
///   - dot                `Symbol.iterator`
///   - string bracket     `Symbol["iterator"]`               (#6676)
///   - optional dot       `Symbol?.iterator`                 (#6719)
///   - optional bracket   `Symbol?.["iterator"]`             (#6719)
///
/// The *runtime*-key computed form `Symbol[name]` / `Symbol?.[name]` (esbuild's
/// `__knownSymbol` helper) can't be constant-folded, so it lowers to a
/// `js_symbol_computed_member` call — a strict superset of a plain `Symbol[key]`
/// read (maps a well-known name to the cached symbol, else reads normally).
///
/// Returns `Ok(None)` unless this is a well-known-symbol read on the real
/// (unshadowed) `Symbol` global, so the caller falls through to its ordinary
/// member lowering. `Symbol` is a non-nullish global, so the optional-chain
/// forms resolve identically to their non-optional twins — the `?.`
/// short-circuit is dead and the caller may drop it.
pub(crate) fn try_fold_symbol_well_known_member(
    ctx: &mut LoweringContext,
    obj: &ast::Expr,
    prop: &ast::MemberProp,
) -> Result<Option<Expr>> {
    let ast::Expr::Ident(obj_ident) = unwrap_transparent(obj) else {
        return Ok(None);
    };
    // Gated on `Symbol` being the real, unshadowed global — a `let`/`const`,
    // `function`, `class`, or imported binding of that name resolves normally,
    // matching JS scoping. `shadows_unqualified_global` covers all four forms
    // (`lookup_local` alone would miss `class Symbol` / `function Symbol`).
    if obj_ident.sym.as_ref() != "Symbol" || ctx.shadows_unqualified_global("Symbol") {
        return Ok(None);
    }
    let fold =
        |name: &str| Expr::SymbolFor(Box::new(Expr::String(format!("@@__perry_wk_{}", name))));
    match prop {
        ast::MemberProp::Ident(prop_ident) => {
            let prop_name = prop_ident.sym.as_ref();
            if is_well_known_symbol_member(prop_name) {
                return Ok(Some(fold(prop_name)));
            }
        }
        ast::MemberProp::Computed(computed) => match computed.expr.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(s)) => {
                if let Some(prop_name) = s.value.as_str() {
                    if is_well_known_symbol_member(prop_name) {
                        return Ok(Some(fold(prop_name)));
                    }
                }
            }
            _ => {
                let key_expr = lower_expr(ctx, &computed.expr)?;
                return Ok(Some(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_symbol_computed_member".to_string(),
                        param_types: vec![Type::Any, Type::Any],
                        return_type: Type::Any,
                    }),
                    args: vec![
                        Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(Expr::GlobalGet(0)),
                            property: "Symbol".to_string(),
                        },
                        key_expr,
                    ],
                    type_args: vec![],
                    byte_offset: 0,
                }));
            }
        },
        ast::MemberProp::PrivateName(_) => {}
    }
    Ok(None)
}

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
                // #2768: read the property off the RUNTIME `new.target`, which
                // codegen resolves to the active constructor's leaf class ref
                // (`INT32_TAG | class_id`). `.name` / `.prototype` /
                // `=== SomeClass` then all reflect the actual constructed
                // class. The old fold returned the *enclosing* class name
                // string (wrong leaf for `super()`-inlined bodies) and made
                // `new.target.prototype` undefined. Outside a constructor
                // `new.target` is `undefined`, so the runtime read yields
                // `undefined.<prop>` semantics via the same PropertyGet.
                return Ok(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::NewTarget),
                    property: prop_name.to_string(),
                });
            }
        }
    }

    // #6560: the `Bun` global shim pack — member position only, and only when
    // no user binding shadows `Bun`. `Bun.stdin` / `Bun.stdout` / `Bun.stderr`
    // are object-valued handle reads; bare method-value reads
    // (`const sw = Bun.stringWidth`) bind the callable export through the
    // native-module property path. Bare `Bun` / `typeof Bun` are deliberately
    // untouched: node-targeting bundles feature-detect Bun exactly that way
    // and must keep taking their node paths. The call form
    // (`Bun.stringWidth(...)`) lowers via `expr_call/module_static.rs`.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Bun"
            && ctx.lookup_local("Bun").is_none()
            && ctx.lookup_native_module("Bun").is_none()
        {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                if matches!(
                    prop_ident.sym.as_ref(),
                    "stdin"
                        | "stdout"
                        | "stderr"
                        | "stringWidth"
                        | "hash"
                        | "file"
                        | "write"
                        | "pathToFileURL"
                        | "fileURLToPath"
                ) {
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::NativeModuleRef("bun".to_string())),
                        property: prop_ident.sym.as_ref().to_string(),
                    });
                }
            }
        }
    }

    // Promise statics are receiver-sensitive: ECMA-262 uses their `this`
    // value as the constructor, so value reads like `Promise.resolve.call(...)`
    // must keep the `Promise` receiver instead of collapsing to the legacy
    // property-only `GlobalGet(0).resolve` intrinsic shape.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let promise_is_source_bound = ctx.lookup_local("Promise").is_some()
            || ctx.lookup_func("Promise").is_some()
            || ctx.lookup_imported_func("Promise").is_some();
        if obj_ident.sym.as_ref() == "Promise" && !promise_is_source_bound {
            let static_member = match &member.prop {
                ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.as_ref()),
                ast::MemberProp::Computed(computed) => match computed.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                    _ => None,
                },
                ast::MemberProp::PrivateName(_) => None,
            };
            if let Some(static_member) = static_member {
                if crate::analysis::is_builtin_static_function_member("Promise", static_member) {
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(Expr::GlobalGet(0)),
                            property: "Promise".to_string(),
                        }),
                        property: static_member.to_string(),
                    });
                }
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
                            byte_offset: 0,
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
                        byte_offset: 0,
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
                            byte_offset: 0,
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
                            byte_offset: 0,
                            object: Box::new(Expr::GlobalGet(0)),
                            property: prop_ident.sym.to_string(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // `Symbol.<well-known>` in every form — dot (`Symbol.iterator`), string
    // bracket (`Symbol["iterator"]`, #6676), and the runtime-key computed form
    // (`Symbol[name]`, esbuild's `__knownSymbol`) — folds to the
    // `@@__perry_wk_<name>` sentinel (or a `js_symbol_computed_member` call) via
    // the shared resolver. See `try_fold_symbol_well_known_member`.
    if let Some(folded) = try_fold_symbol_well_known_member(ctx, member.obj.as_ref(), &member.prop)?
    {
        return Ok(folded);
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
                                // #5009: honor a build-time `perry.define` of
                                // `process.env.<NAME>` by substituting the
                                // literal here — before `EnvGet` (a live
                                // runtime env lookup) is emitted. esbuild-style
                                // define semantics: the define wins over the
                                // runtime environment, in every context and
                                // regardless of tree-shaking.
                                if let Some(lit) = env_define_literal(&var_name) {
                                    return Ok(lit);
                                }
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
                                        // #5009: same define substitution as the
                                        // dot form above.
                                        if let Some(lit) = env_define_literal(name) {
                                            return Ok(lit);
                                        }
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
            if let Some(prop_name) = static_member_prop_name(&member.prop) {
                let val = match prop_name.as_str() {
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
            if let Some(prop_name) = static_member_prop_name(&member.prop) {
                let val = match prop_name.as_str() {
                    "MAX_SAFE_INTEGER" => Some(9007199254740991.0),
                    "MIN_SAFE_INTEGER" => Some(-9007199254740991.0),
                    "MAX_VALUE" => Some(f64::MAX),
                    // smallest denormal (5e-324), not smallest normal
                    "MIN_VALUE" => Some(f64::from_bits(1)),
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

    // Computed access on an enum identifier: `Color[expr]` (#4509).
    // TypeScript numeric enums carry a reverse mapping in addition to the
    // forward one — `Color.Blue === 2` *and* `Color[2] === "Blue"`. The
    // `.Member` form above folds to a compile-time constant, but the
    // computed form can index with a runtime value, so materialize the
    // enum's runtime object — the forward members plus a reverse entry for
    // every numeric member — and index into it. String-valued members get
    // no reverse entry, matching tsc (string enums are one-directional).
    // Unwrap TS-only casts/parens so `(Color as any)[c]` is recognised too.
    {
        fn unwrap_enum_receiver(mut e: &ast::Expr) -> &ast::Expr {
            loop {
                match e {
                    ast::Expr::TsAs(x) => e = &x.expr,
                    ast::Expr::TsNonNull(x) => e = &x.expr,
                    ast::Expr::TsConstAssertion(x) => e = &x.expr,
                    ast::Expr::TsTypeAssertion(x) => e = &x.expr,
                    ast::Expr::TsSatisfies(x) => e = &x.expr,
                    ast::Expr::Paren(x) => e = &x.expr,
                    _ => break,
                }
            }
            e
        }
        if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Computed(computed)) =
            (unwrap_enum_receiver(member.obj.as_ref()), &member.prop)
        {
            let members: Option<Vec<(String, crate::ir::EnumValue)>> = ctx
                .lookup_enum(obj_ident.sym.as_ref())
                .map(|(_, m)| m.to_vec());
            if let Some(members) = members {
                let index = lower_expr(ctx, &computed.expr)?;
                let mut fields: Vec<(String, Expr)> = Vec::new();
                for (name, value) in &members {
                    match value {
                        crate::ir::EnumValue::Number(n) => {
                            fields.push((name.clone(), Expr::Number(*n as f64)));
                            fields.push((n.to_string(), Expr::String(name.clone())));
                        }
                        crate::ir::EnumValue::String(s) => {
                            fields.push((name.clone(), Expr::String(s.clone())));
                        }
                    }
                }
                return Ok(Expr::IndexGet {
                    object: Box::new(Expr::Object(fields)),
                    index: Box::new(index),
                });
            }
        }
    }

    // Check if this is a static field access (e.g., Counter.count).
    // #5938 follow-up: resolve scope-local class renames first — a
    // body-local colliding `class X` registers under `class_renames`, and
    // the raw name would bind the FIRST same-named registrant's statics.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = ctx.resolve_class_name(obj_ident.sym.as_ref());
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
                            byte_offset: 0,
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
                // #wall (follow-redirects): JS object-metadata / prototype-chain
                // properties are never native instance methods/getters. Reading
                // `inst.prototype` / `inst.__proto__` / `inst.constructor` on a
                // value the HIR tagged as a native instance (e.g. `const lN =
                // Readable.from(...)`) must NOT route to the 0-arg
                // NativeMethodCall fallback below — that lowers to
                // `js_native_call_method_nullsafe(inst, "prototype", 0 args)`,
                // which *invokes* the resolved value → `TypeError: prototype is
                // not a function`. Node returns the real metadata value (e.g.
                // `undefined` for an instance's `.prototype`). Lower these as a
                // plain PropertyGet so the runtime reads the property instead of
                // calling it. (`constructor` for the bare-stream classes is also
                // remapped to the module export above; this catches the general
                // case for every other native-instance module/class.)
                if matches!(
                    property_name.as_str(),
                    "prototype" | "__proto__" | "constructor"
                ) {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                }
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
                    // Issue #562 + #wall (debug `_.colors`): a Web-Streams
                    // instance / subclass carries the bare-stream tag for
                    // inherited-method dispatch but ALSO holds user-declared
                    // own fields (`w.seenLengths`, `x.colors = [...]`). A bare
                    // read of any name that is NOT a known Web-Streams API
                    // method/getter must be a plain own-property GET — NOT the
                    // 0-arg NativeMethodCall fallback (which would *invoke* the
                    // stored value). The matching write lowers to a generic
                    // PropertySet that the runtime routes to the per-handle
                    // expando side-table (`object/handle_expando.rs`), so the
                    // value persists and reads back. Real stream methods/getters
                    // (handled by `is_stream_api_member` above) keep dispatching.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "blob" && !is_blob_getter_name(&property_name) {
                    // #wall (debug `_.colors`): a Blob/File instance is a real
                    // heap object that also carries user-assigned OWN properties
                    // (library bookkeeping fields, `x.colors = [...]`, etc.). A
                    // bare read of any name that is NOT a known native data
                    // getter (`size`/`type`/`name`/`lastModified`, handled by the
                    // generic fallback's 0-arg NativeMethodCall → FFI dispatch)
                    // must be a plain own-property GET — NOT the invoking
                    // fallback, which lowers to `js_native_call_method_nullsafe(
                    // inst, "<name>", 0 args)` and *calls* the stored value (an
                    // array → `TypeError: value is not a function`). Real Blob
                    // *methods* (`x.text()`, `x.slice()`, `x.arrayBuffer()`)
                    // arrive through the call-expression path, not this bare
                    // read, so they still dispatch.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "fetch" && !is_fetch_response_getter_name(&property_name) {
                    // Same heap-object rule for fetch `Response` instances: an
                    // arbitrary property read that is not a known Response data
                    // getter (`status`/`ok`/`headers`/…) must be a plain
                    // own-property GET, not an invoking 0-arg native call. Body
                    // methods (`res.json()`/`res.text()`/`res.clone()`) come in
                    // via the call path and keep dispatching.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "util" | "sys")
                    && matches!(class_name.as_str(), "MIMEType" | "MIMEParams")
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "url"
                    && class_name == "URLPattern"
                    && is_url_pattern_data_property(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "stream" | "node:stream")
                    && is_classic_stream_method_name(&property_name)
                {
                    // Classic Node streams materialize core stream and
                    // EventEmitter methods as closure-valued fields on the
                    // stream object. A bare method read (`r.read`, `r.on`)
                    // must return that callable value, not invoke the native
                    // receiver method with no args.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "stream" | "node:stream")
                    && !is_classic_stream_getter_name(&property_name)
                {
                    // #wall (debug `_.colors`): a classic Node stream instance
                    // is a heap object that also carries user-assigned OWN
                    // properties (`_.colors = [...]`, library bookkeeping fields,
                    // etc.). A bare read of any name that is NOT a known stream
                    // method (handled by the arm above) and NOT a known stream
                    // property getter (the allowlist below) must be a plain
                    // own-property GET — NOT the 0-arg NativeMethodCall fallback,
                    // which lowers to `js_native_call_method_nullsafe(inst,
                    // "<name>", 0 args)` and *invokes* the stored value
                    // (`_.colors` is an array → `TypeError: value is not a
                    // function`). Node just returns the property. The matching
                    // write (`_.colors = v`) already lowers to a generic
                    // PropertySet on the heap object, so the value persists and
                    // reads back. Known getters (`destroyed`, `readableLength`,
                    // …) still keep the NativeMethodCall fallback below so the
                    // codegen NativeModSig table dispatches them to their FFI.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "events"
                    // Any native instance registered under module `events` is
                    // an emitter; the class name can be an alias of the
                    // constructor binding rather than the canonical
                    // "EventEmitter" — `var EE = require('events'); new EE()`
                    // registers class "EE" (#4995). Gating on the canonical
                    // names sent alias reads down the zero-arg
                    // NativeMethodCall path, so `typeof emitter.on` CALLED
                    // `events.on()` and threw ERR_INVALID_ARG_TYPE.
                    && (matches!(
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
                        && property_name == "emitDestroy"))
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if class_name == "AsyncLocalStorage"
                    && matches!(
                        property_name.as_str(),
                        "run" | "getStore" | "enterWith" | "exit" | "disable"
                    )
                {
                    // `als.getStore` / `als.run` etc. are method-VALUE reads,
                    // not zero-arg native calls. A bare read (`const { getStore
                    // } = als`, `const gs = als.getStore`, `typeof als.getStore`
                    // — Next.js' cacheComponents / patch-fetch async-storage
                    // setup) must return the callable BOUND METHOD, not invoke
                    // `getStore()` with no args (which returns the store →
                    // undefined → `TypeError: getStore is not a function` at
                    // server startup, before `✓ Ready`). Keep PropertyGet so the
                    // runtime handle-property dispatch
                    // (`dispatch_async_local_storage_property`) binds the method;
                    // the call form `als.getStore()` still dispatches via the
                    // runtime handle method dispatch. Mirrors the EventEmitter /
                    // Console / net.Socket method-value-read arms above.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "Agent"
                    && property_name == "close"
                {
                    return Ok(Expr::Undefined);
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "Agent"
                    && matches!(
                        property_name.as_str(),
                        "keepSocketAlive" | "reuseSocket" | "getName" | "destroy"
                    )
                {
                    // A bare read of an Agent method (`typeof a.getName`)
                    // should produce a callable bound-method value, not invoke
                    // the native method with zero arguments.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "sqlite"
                    && class_name == "Session"
                    && matches!(property_name.as_str(), "changeset" | "patchset" | "close")
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "Headers" && is_headers_method_name(&property_name) {
                    // A bare Fetch Headers method read (`headers.entries`) is a
                    // function value, not a zero-arg native call. The call form
                    // (`headers.entries()`) is handled by expr_call lowering.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                        byte_offset: 0,
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
                    "locked"
                        | "desiredSize"
                        | "closed"
                        | "ready"
                        | "readable"
                        | "writable"
                        | "byobRequest"
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
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ClientRequest"
                    && is_http_client_request_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if module_name == "http"
                    && class_name == "IncomingMessage"
                    && is_http_incoming_message_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "IncomingMessage"
                    && is_http_incoming_message_runtime_property_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ServerResponse"
                    && is_http_server_response_method_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if matches!(module_name.as_str(), "http" | "https")
                    && class_name == "ServerResponse"
                    && is_http_server_response_runtime_property_name(&property_name)
                {
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
                    });
                } else if is_native_dispatch_member(&module_name, &class_name, &property_name) {
                    // #wall (debug `_.colors` / `_.init`): INVERTED DEFAULT.
                    // A bare native-instance member READ now defaults to a
                    // plain `PropertyGet` (the final `else` below). It only
                    // reaches this INVOKING `NativeMethodCall { args: [] }`
                    // dispatch path when the property is a *known* native
                    // method/getter that must dispatch through the codegen
                    // NATIVE_MODULE_TABLE / per-class FFI
                    // (`is_native_dispatch_member`). Previously this arm was
                    // the unconditional catch-all `else`, so any value the
                    // HIR mis-tagged native under a module NOT covered by the
                    // per-module arms above (the bundled `debug` package's
                    // `createDebug` value) had EVERY non-method property read
                    // lowered to a 0-arg native call that *invoked* the
                    // stored value — `_.colors` (an array) →
                    // `value is not a function`; `_.init` (a function) called
                    // with 0 args → `Cannot set properties of null`. Real
                    // native method calls `x.method(args)` arrive via the
                    // call-expression path (local_natives.rs / lower_call),
                    // NOT this bare-read block, so they are unaffected.
                    //
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
                            | ("IncomingMessage", "httpVersionMajor")
                            | ("IncomingMessage", "httpVersionMinor")
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
                            | ("HttpServer", "listening")
                            | ("HttpServer", "headersTimeout")
                            | ("HttpServer", "keepAliveTimeout")
                            | ("HttpServer", "keepAliveTimeoutBuffer")
                            | ("HttpServer", "requestTimeout")
                            | ("HttpServer", "timeout")
                            | ("HttpServer", "maxHeadersCount")
                            | ("HttpServer", "maxRequestsPerSocket")
                            | ("HttpsServer", "listening")
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
                        if matches!(module_name.as_str(), "http" | "https" | "events" | "net") {
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
                } else {
                    // INVERTED DEFAULT (#wall debug `_.colors` / `_.init`): a
                    // bare native-instance member read that is NOT a known
                    // native dispatch member is a plain own-property GET — it
                    // READS the stored value instead of INVOKING it as a 0-arg
                    // native method call. This is what makes a value the HIR
                    // mis-tagged native under an uncovered module (the bundled
                    // `debug` package's `createDebug`) read `_.colors` / `_.init`
                    // as ordinary properties (Node's behaviour) rather than
                    // calling them. Writes to native handles already lower to a
                    // generic PropertySet routed through the per-handle expando
                    // side-table (`object/handle_expando.rs`), so the value
                    // persists and reads back here.
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(object_expr),
                        property: property_name,
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
        // RegExpExecArray `.index` / `.groups` / `.input` are NOT folded to
        // thread-local reads: the runtime attaches them as real own properties
        // on each exec/match result array (regex.rs::set_exec_array_metadata /
        // set_exec_array_groups), so a generic PropertyGet reads the per-result
        // value. That keeps a stored `m.index` / `m.groups` correct after an
        // intervening match on another regex, where a thread-local was clobbered.
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

    // Perf: reuse a receiver already lowered by `try_static_method_and_instance`
    // (the chained-native-method dispatch helper) for THIS exact member callee,
    // instead of re-lowering the whole prefix. See
    // `LoweringContext::prelowered_member_receiver`. Match strictly by span and
    // take it (single-shot) so a stale memo can never leak onto a different
    // receiver. Any other consumer along the way invalidates it.
    // Tail (chore/split-large-files): receiver lowering + builtin-static
    // reroute-undo + .name/.length folds + #463 gate + final dispatch.
    lower_member_tail(ctx, member, member_is_call_callee)
}
