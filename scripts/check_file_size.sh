#!/usr/bin/env bash
#
# CI gate: fail if any tracked Rust source file exceeds the LOC threshold.
#
# Big single-file modules are hard to read, hard to review, and hurt
# build incrementality (touching one symbol invalidates the IDE +
# cargo-check work for thousands of lines downstream). This script
# enforces an upper bound and is run on every PR.
#
# Threshold is **2,000 lines** as of v0.5.1020. Started at 5,000 in
# v0.5.1019 with the first wave of splits (compile.rs / expr/mod.rs /
# native_table.rs / etc.), tightened to 2,000 once the long-tail
# 2k-5k files were split topically (lower_decl/, inline/, json/,
# stable_hash/, builtins/, array/, monomorph/, publish/, arena/,
# emit/, generator/, js_transform/, modules/, run/, promise/, setup/,
# string/, ir/, runtime_decls/, value/, perry-ui-{macos,ios,android,
# visionos,tvos,windows,gtk4}/, closure/, walker/, dispatch/, lower/,
# buffer/, destructuring/, lower_call/native/, interop/, stmt/, url/,
# bridge/, deforest/, compile/link/, compile/cjs_wrap/, …).
#
# Scope: only checks `*.rs` files. Other formats (JS runtime
# templates, HTML examples, Kotlin templates, JSON fixtures, dist
# bundles) intentionally not policed — they aren't really "review
# surface" the way production Rust is.
#
# Allowlisted (real Rust source, deferred for a specific reason —
# **each entry needs a one-line rationale**):
#
#   - crates/perry-runtime/src/gc/tests.rs — left behind by the gc.rs
#     split in the #1090 GC architecture checkpoint. The companion
#     production files in `gc/` all came in under 2k; only the test
#     fixture remained big. Re-evaluate once the GC owner peels it
#     apart.
#   - crates/perry-codegen-arkts/src/tests.rs — ArkTS golden-output
#     test fixtures. Top-down test scaffolding, not production code;
#     splitting would split assertions away from the inputs that
#     produced them.
#   - crates/perry-api-manifest/src/entries.rs — generated-feel
#     manifest table (one entry per public API surface item). Length
#     reflects API breadth, not complexity, and splitting would scatter
#     entries that ought to live next to each other for drift review.
#   - crates/perry/src/commands/compile.rs — the deeply-coupled
#     `par_iter` codegen closure inside `run_with_parse_cache`
#     (~1,800 LOC, ~30 captured locals) needs extraction into a
#     context-struct helper. High-risk surgery deferred to a
#     follow-up PR; the rest of compile.rs was already split into
#     compile/{types,bootstrap,bundle_apple,...} sub-modules
#     (16 siblings in compile/).
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-2000}"

# Allowlist (one file per line; blank lines + `#` comments OK).
ALLOWLIST=$(cat <<'EOF'
crates/perry-runtime/src/gc/tests.rs
crates/perry-codegen-arkts/src/tests.rs
crates/perry-api-manifest/src/entries.rs
crates/perry/src/commands/compile.rs
# `PERRY_UI_TABLE` — flat `MethodRow` data table for receiver-less perry/ui
# calls (one row per constructor/setter). Generated-feel manifest like
# entries.rs: length reflects widget-API breadth, not complexity, and a single
# const array can't be split across files without scattering rows that belong
# next to each other for review. Crossed 2000 LOC on current main.
crates/perry-dispatch/src/ui_table.rs
# Native-module dispatch table; one big match by (module, method, class).
# Splitting per-namespace is tracked under the API-manifest refactor in #793.
crates/perry-codegen/src/lower_call/native/mod.rs
# Node-core native method table split out of `native/mod.rs`; still a single
# per-module dispatch table and already over the limit on current main.
# Splitting per namespace is tracked under the codegen cleanup in #1435.
crates/perry-codegen/src/lower_call/native_table/node_core.rs
# HIR `Expr` enum + dependency-walker arms; splitting would need parallel
# updates across every variant of the walker traits. Tracked alongside #793.
crates/perry-hir/src/ir/expr.rs
# HIR member-expression lowering tower; already over the line-count threshold
# on main after the process allowed-flags additions. Split by member family is
# tracked alongside the lower/codegen file-size cleanup in #1435.
crates/perry-hir/src/lower/expr_member.rs
# Object field get/set + handle/native dispatch shim; grew past the limit
# after the #1419 KeyObject/.export/.equals routing + main's process-module
# additions. Splitting tracked under #1435.
crates/perry-runtime/src/object/field_get_set.rs
# Codegen `Call` dispatch tower; grew past the limit after #1419's crypto
# fast-path gate refinements + main's process / fs / perf_hooks Expr
# additions. Splitting per-builtin family tracked alongside #1435.
crates/perry-codegen/src/expr/calls.rs
# Codegen `Expr` lowering trunk (one big match over every `Expr` variant);
# crossed the limit on current main after recent builtin-Expr additions.
# Splitting per expression family is tracked alongside #1435.
crates/perry-codegen/src/expr/mod.rs
# Dynamic method-call dispatch tower (js_native_call_method); crossed the
# limit by the 7-line WeakMap/WeakSet dispatch hook in #1757/#1758. Splitting
# per receiver-kind family tracked alongside #1435.
crates/perry-runtime/src/object/native_call_method.rs
# Codegen driver: `compile_module` is a single ~2,100-LOC function (module
# setup -> per-class field-layout -> per-fn codegen -> link). It crossed the
# limit after #26's cross-module same-named-class disambiguation (the
# class_field_counts / class_init_chains build, interleaved with the per-class
# loop). Extracting that pass is high-risk surgery deferred to the codegen
# split tracked under #1435.
crates/perry-codegen/src/codegen/mod.rs
# Global object bootstrap crossed the gate on current main; split constructor
# tables/population helpers alongside the runtime object cleanup tracked in #1435.
crates/perry-runtime/src/object/global_this.rs
# Central class registry — class IDs, prototypes, parent-closure
# scanning, and field-init replay. Crossed the limit after the
# #1787 instance-field init replay (#2074) + web-stream class
# wiring (#1641/#2110). Split tracked under #1435.
crates/perry-runtime/src/object/class_registry.rs
# Global object/bootstrap native singleton table crossed the current-main
# threshold after recent builtin surface additions. Splitting constructor and
# singleton installers into sibling modules is tracked under #1435.
crates/perry-runtime/src/object/global_this.rs
# Native-module namespace property/method dispatcher
# (`get_native_module_constant` is one big match — one arm per
# stdlib namespace, every property literal inline). Splitting per
# namespace would scatter arms that share helpers (`fs_const`,
# `os_signal_const`, …) and the constants tables they index.
# Crossed the limit at 2014 LOC after the #2135 worker_threads
# value-export arm. Split tracked under #1435.
crates/perry-runtime/src/object/native_module.rs
# globalThis constructor/namespace registry; current main crossed the threshold
# after WebCrypto + DOM/Event global exposure landed. Split tracked under #1435.
crates/perry-runtime/src/object/global_this.rs
# Node core native-lowering table; current main crossed the threshold after
# namespace alias exposure work. Split tracked under #1435.
crates/perry-codegen/src/lower_call/native_table/node_core.rs
# fs directory glob/watch helpers; current main crossed the threshold after
# namespace-alias exposure work. Split tracked under #1435 with the other
# runtime file-size cleanups.
crates/perry-runtime/src/fs/dir_glob_watch.rs
# node:fs module root — crossed the gate after the final fs parity
# surface reconciliation (#3969) bumped its dispatch tower by a few lines.
# Splitting tracked under #1435 with the other runtime file-size cleanups.
crates/perry-runtime/src/fs/mod.rs
# stdlib native dispatch table; current main crossed the threshold after
# namespace-alias exposure work. Split tracked under #1435.
crates/perry-stdlib/src/common/dispatch.rs
# SQLite stdlib shim remains a generated-feel native adapter table; current
# main crossed the threshold before this PR. Split tracked under #1435.
crates/perry-stdlib/src/sqlite.rs
# Member-expression lowering tower (one big match over member/property/call
# shapes, plus per-namespace literal builders). Crossed the limit at 2121 LOC
# after #3161 inlined the full allowedNodeEnvironmentFlags string list into
# `process_allowed_node_flags_literal`. Splitting the per-namespace literal
# builders into a sibling module is tracked under #1435.
crates/perry-hir/src/lower/expr_member.rs
# Built-in call intrinsic-lowering tower. Crossed the 2000-line gate on current
# main after the String.prototype generic-`this` + Array/Promise receiver-brand
# parity arms (#4713/#4720/#4603). Splitting the per-builtin lowering helpers
# into sibling modules is tracked under #1435.
crates/perry-hir/src/lower/expr_call/intrinsics.rs
# Expression lowering entry point — crossed the 2000-line gate when the
# CJS-default-import allow-list grew to cover all node-core namespaces with
# `default` namespace shims (#3903). Splitting the per-namespace dispatch
# helpers into a sibling module is tracked under #1435.
crates/perry-hir/src/lower/lower_expr.rs
# Module-declaration lowering tower (import/export binding resolution, re-export
# wiring, namespace shims). Crossed the 2000-line gate after exported
# destructuring-binding support added the pattern-walk arms. Splitting the
# export-binding helpers into a sibling module is tracked under #1435.
crates/perry-hir/src/lower/module_decl.rs
# Bare-callee intrinsics + CJS/UMD legacy-shape lowering (require/eval/Function
# folds, IIFE rewrite, RegExp bare-call). Crossed the 2000-line gate (2010 LOC)
# on current main, independent of this PR. Splitting the per-shape helpers into a
# sibling module is tracked under #1435.
crates/perry-hir/src/lower/expr_call/intrinsics.rs
# node:process surface (env/argv/hrtime/cpuUsage/resourceUsage + EventEmitter
# wiring + warning/deprecation emit). Crossed the limit at 2047 LOC after the
# argument-validation batch landed on main without a split (#3493 setuid/setgid/
# umask, #3516 exit/chdir/hrtime/cpuUsage, #3518 warning events, #3496 CPU-
# snapshot/listener-limit validation). Splitting per concern (env/timing/
# signals/emitter) is tracked under #1435.
crates/perry-runtime/src/process.rs
# fs directory glob/watch glue crossed the gate on current main; split glob
# walking from watcher dispatch alongside the fs modularization tracked in #1435.
crates/perry-runtime/src/fs/dir_glob_watch.rs
# Shared stdlib dispatch bridge crossed the gate on current main; split per
# dispatch family with the stdlib dispatch cleanup tracked in #1435.
crates/perry-stdlib/src/common/dispatch.rs
# sqlite stdlib remains a monolithic binding surface on current main; split
# statements/sessions/backups/functions in the sqlite cleanup tracked in #1435.
crates/perry-stdlib/src/sqlite.rs
# Node core native table crossed the limit on current main after namespace
# alias additions; split per namespace in the native-table cleanup tracked in #1435.
crates/perry-codegen/src/lower_call/native_table/node_core.rs
# HTTP/HTTPS native table crossed the limit on current main after ClientRequest
# header-state surface additions; split per client/server family in the
# native-table cleanup tracked under #1435.
crates/perry-codegen/src/lower_call/native_table/http.rs
# globalThis constructor/prototype registry is over the limit on current main;
# splitting constructor tables from property dispatch is tracked under #1435.
crates/perry-runtime/src/object/global_this.rs
# Trunk of the #1103 object.rs split (shape/transition/overflow caches, GC root
# scanners, implicit-this, descriptor tables). The companion behavior lives in
# the 30+ `object/` siblings already peeled off; the trunk crossed 2000 LOC on
# current main after the binary-data / Date inspect alignment batch
# (#4039/#4040/#4041). Peeling the cache + root-scanner groups into siblings is
# tracked under #1435.
crates/perry-runtime/src/object/mod.rs
# Symbol subsystem (Symbol primitives + per-object/per-class symbol-keyed
# property + accessor side tables, with their GC root-scan/rewrite dispatch).
# Crossed the limit at 2159 LOC after the computed-property-names batch added
# symbol-accessor descriptors and class-static computed-symbol registration
# (#3557/#3558/#3559/#3560/#3561). The new helpers are interwoven with the
# symbol root scanner, so a clean topical split is deferred to the runtime
# file-size cleanup tracked under #1435.
crates/perry-runtime/src/symbol.rs
# Sibling of the #1103 object.rs split (defineProperty/getOwnPropertyNames/
# descriptor + property-ops machinery). Allowlisted on main at 2004 LOC; this
# PR peeled `js_to_property_key`/object-super helpers into property_key.rs and
# `js_create_namespace` into namespace_create.rs to keep it comfortably under
# the gate. Kept here as a backstop in case the merged dispatch tower creeps
# back over; further descriptor/ops splits are tracked under #1435.
crates/perry-runtime/src/object/object_ops.rs
# node:http/https native-lowering table (one dispatch arm per ClientRequest /
# IncomingMessage / ServerResponse member). Crossed the 2000-line gate after the
# http live-message + ClientRequest header-state surface additions (#4152/#4159).
# Splitting per message-kind family is tracked under #1435.
crates/perry-codegen/src/lower_call/native_table/http.rs
# child_process module root (spawn/exec/fork dispatch + reactor wiring). Crossed
# the 2000-line gate after the stdio `'ignore'` handling additions. Splitting the
# spawn/exec/fork families into sibling modules is tracked under #1435.
crates/perry-runtime/src/child_process/mod.rs
# OCI container backend (docker/podman/apple-container process orchestration +
# OCI lifecycle: create/start/stop/exec/logs/inspect/image ops). Lands oversized
# from the container-compose subsystem (replacement for external PR #159); the
# backend is gated behind the `container` feature. Splitting per backend driver
# / lifecycle family is tracked under #1435.
crates/perry-container-compose/src/backend.rs
# perry-stdlib container module root — re-exports `perry_container_compose::*`
# and the `js_container_*` / `js_compose_*` FFI dispatch surface (gated behind
# the `container` feature). Splitting the FFI surface per command family is
# tracked under #1435.
crates/perry-stdlib/src/container/mod.rs
# HIR analysis pass (binding/closure/this-capture + builtin-shape analysis).
# Crossed the 2000-line gate after the prototype/super assignment parity arms.
# Splitting per analysis concern is tracked under #1435.
crates/perry-hir/src/analysis.rs
# node:stream classic constructor + web-adapter surface (Readable/Writable/
# Duplex/Transform construction + toWeb/fromWeb/Readable.fromWeb adapters).
# Crossed the 2000-line gate after the stream/web adapter additions. Splitting
# classic constructors from the web adapters is tracked under #1435.
crates/perry-runtime/src/node_stream_constructors.rs
# Codegen property-get / method-dispatch lowering tower (one arm per builtin
# accessor + collection/string/regex method). Crossed the 2000-line gate (2004
# LOC) on main after recent dispatch-arm additions. Splitting per receiver-type
# family is tracked under #1435.
crates/perry-codegen/src/expr/property_get.rs
# Codegen call-site method-dispatch tower (string/array/class/Map/Set/Promise +
# static/instance method resolution). Sat at 1998 LOC on main; crossed the
# 2000-line gate after the class static-accessor call route (test262
# arguments-object cls-*-static-* getter calls). Splitting the per-receiver-type
# dispatch helpers into sibling modules is tracked under #1435.
crates/perry-codegen/src/lower_call/property_get.rs
# TypedArray root — constructor/view-metadata/element load-store/iterator tower.
# Crossed the 2000-line gate (2062 LOC) on current main after the #4702
# %TypedArray%.prototype iterator brand-check + array-like/iterable constructor
# additions. Splitting per concern is tracked under #1435.
crates/perry-runtime/src/typedarray/mod.rs
# Generator/async-generator state-machine lowering core (linearize → states →
# next/return/throw step closures + async-step driver). Crossed the 2000-line
# gate after the standalone async-generator parity work: synchronous param-
# prologue lift (run param binding at call time) + per-yield operand Await.
# Splitting the state builder from the closure assembly is tracked under #1435.
crates/perry-transform/src/generator/lower.rs
EOF
)

# Anchor at repo root so the script can be invoked from anywhere.
cd "$(git rev-parse --show-toplevel)"

# Build the offender list — tracked Rust files only.
violations=""
total=0
while IFS= read -r f; do
    [ -f "$f" ] || continue

    # Allowlist match.
    if grep -Fxq "$f" <<<"$ALLOWLIST"; then continue; fi

    lines=$(wc -l < "$f" 2>/dev/null || echo 0)
    if [ "$lines" -gt "$THRESHOLD" ]; then
        violations+="$(printf '%7d  %s\n' "$lines" "$f")"$'\n'
        total=$((total + 1))
    fi
done < <(git ls-files '*.rs')

if [ "$total" -gt 0 ]; then
    echo "::error::File size limit exceeded ($THRESHOLD lines)."
    echo ""
    echo "The following files are too large:"
    echo "$violations"
    echo ""
    echo "Split the offending files into topical sub-modules. See"
    echo "v0.5.1019/v0.5.1020 commits on chore/split-large-files for"
    echo "the recipe: extract function groups into sibling files,"
    echo "re-export from mod.rs with explicit named use statements"
    echo "(globs don't propagate through transitive re-exports). To"
    echo "deliberately exclude a file (e.g. a refactor in progress"
    echo "tracked elsewhere) add it to the ALLOWLIST block at the top"
    echo "of this script with a one-line rationale."
    exit 1
fi

echo "OK: no Rust source files exceed $THRESHOLD lines."
