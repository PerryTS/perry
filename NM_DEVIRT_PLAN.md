# Native-module method-dispatch devirtualization (feat/nm-method-devirt)

Goal: let `-dead_strip` remove native-module handler code (cluster/child_process/
dns/tls/vm/repl/inspector/perf_hooks/sqlite/dgram/…) from binaries that don't
import those modules. Today one monolithic `dispatch_native_module_method`
(437→592 arms, `native_module_dispatch.rs`) statically names every handler, so any
program creating one native namespace pins all of them. Measured ceiling for
hello-world: ~213KB (method dispatch only; constructor dispatcher in
class_registry.rs is a SEPARATE later phase).

## Validated partition
592 arms → 37 buckets, 0 unmapped, 0 unbalanced (brace-balanced boundaries).
Buckets: assert async_hooks bigint buffer child_process cluster console crypto
dgram dns domain events fs http https inspector module net os path perf process
punycode querystring readline repl sea sqlite stream timers tls tty url util v8
vm wasi zlib. Sub-namespace tags map to bucket: crypto.subtle/webcrypto/Certificate→crypto,
path.posix/win32→path, util.types/util/types→util, dns/promises→dns,
assert/strict & assert.instance→assert, inspector/promises & inspector.Network→inspector,
punycode.default→punycode, perf_hooks/perf_observer*/perf_histogram→perf,
v8.Serializer/Deserializer/GCProfiler/promiseHooks/startupSnapshot→v8.

## Design (minimal disruption — vtable struct + hook UNCHANGED)
- `NmCtx { obj, args_ptr, args_len, assert_skip_prototype }` + `nm_general_closures!`
  macro (general closures only: arg,i32_arg,str_to_f64,bool_to_f64,bool_tag,ptr_addr,
  optional_ptr_addr,arg_bits,pack_args,pack_args_from,ptr_to_f64,typed_kind,_arg_event_ptr,
  _arg_closure_ptr). Path closures (require_path_str_ptr,optional_path_str_ptr,
  path_join/resolve/basename_value) inline ONLY in nm_dispatch_path.
- `dispatch_native_module_method(obj,method,args,len)` becomes a THIN ROUTER: extract
  field0 name + existing normalization (current lines 128-165) → build NmCtx →
  `nm_dispatch_registry_lookup(canonical) -> Option<fn(&NmCtx,&str,&str)->f64>` →
  call, else undefined. Still pointed at by the shared vtable.dispatch (955) and
  native_arena.rs:474 — both unchanged.
- 37 `nm_dispatch_<b>(ctx,module,method)->f64`: `let NmCtx{obj,args_ptr,args_len,
  assert_skip_prototype}=*ctx; nm_general_closures!(); match (module,method){ <verbatim
  bucket arms> _=>undefined }`.
- Registry (native_module_registry.rs): bucket id enum + `NM_DISPATCH_REGISTRY:
  [AtomicPtr; 37]` (null init) + `nm_module_index(name)->Option<bucket>` (string match,
  NO fn refs) + `#[no_mangle] js_nm_install_<b>()` storing `nm_dispatch_<b> as ptr`
  (SOLE static ref to each bucket fn) + `js_nm_install_all()` (dynamic-require fallback).
- Codegen: at each `js_create_native_module_namespace` site (8 sites, main
  static_field_meta.rs:572) ALSO emit `js_nm_install_<b>()` for the static name, or
  `js_nm_install_all()` if the module name is dynamic/unanalyzable. Runtime-internal
  creators (node_v8, perf_hooks) call their `js_nm_install_v8/perf()`.
- Completeness invariant: every namespace-create site (compile-time name) emits the
  matching install BEFORE any method dispatch on it → registry never misses → never
  silently returns undefined. Dynamic name → install_all (correct, larger).

## Why correct (vs cfg-gating): precise linker reachability through real edges,
sound graceful degradation (install_all), semantics never change with a build flag.

## Status
[x] worktree off origin/main (5258a6073)
[x] partition validated (592 arms → 37 active buckets, https dropped = 0-arm)
[x] generated: NmCtx + nm_general_closures! macro + thin router + 37 nm_dispatch_<b> fns (native_module_dispatch.rs)
[x] registry: NmBucket + NM_DISPATCH_REGISTRY + nm_module_index + 37 js_nm_install_<b>() + js_nm_install_all() (native_module_registry.rs)
[x] **perry-runtime compiles GREEN** (cargo build -p perry-runtime, 0 errors)
[x] CODEGEN: emit js_nm_install_<b>() at all 5 js_create_native_module_namespace sites (nm_install.rs nm_install_symbol; externs declared in runtime_decls/objects.rs). perry builds green.
[x] CORRECTNESS verified byte-identical to node: hello-world, import os, import path, global process (cwd/pid/argv), util.format/inspect/types, querystring, assert.
[x] **MEASURED: hello-world __text 4,667,824 → 4,058,936 = −608,888 B (−13%); binary 5.4MB → 4.7MB.** (baseline = pristine origin/main perry.)
[x] FOLLOW-UP #1 DONE — dynamic getBuiltinModule/require fallback via indirect install-all hook:
    - native_module_get_builtin_module_value → nm_run_install_all_hook() (opaque ptr, names no bucket)
    - js_nm_enable_install_all() (black_box'd, sole ref to js_nm_install_all) armed by js_process_get_builtin_module_devirt (codegen getBuiltinModule table target)
    - black_box REQUIRED: else whole-program opt devirtualizes the single-ptr indirect call → re-pins all (per-bucket array is immune, runtime-indexed).
    - Verified: getBuiltinModule(dynamic+literal), require(literal), global process, static import — all byte-identical to node; hello-world __text 4,058,968 (install_all absent). RESIDUAL EDGE: require(runtimeVar) of builtin (module_require.rs:121, not armed) — narrow, likely deferred anyway.
[x] PHASE 2 DONE (commit 8406fafea) — node-module-namespaced constructor devirt:
    - 8 direct-call ctor blocks (tty/fs/vm/tls/wasi/readline/repl/stream) in js_new_function_construct
      → per-module nm_ctor_<m> fns (class_registry.rs) routed via NM_CTOR_REGISTRY, registered by the
      SAME js_nm_install_<module>() (no new codegen). Globals (URL/WeakSet/Error/TypedArray) stay inline;
      http/events/zlib/sqlite already dynamic-dispatch.
    - Measured: hello-world __text 4,058,968 → 3,971,252 (repl fully stripped). TOTAL from baseline
      4,667,824 → 3,971,252 = −696,572 (−14.9%); binary 5.4MB → ~4.6MB.
    - Correct: new stream.Readable/Writable/Transform, global new URL/TextEncoder/WeakSet/Error/Uint8Array,
      + all 6 phase-1 cases.
[ ] PHASE 3 (diminishing returns) — residual node_stream/tls/child_process/cluster pinned by INTRA-subsystem
    refs (js_node_stream_from_web→readable_new) + method-dispatch internals that construct streams. Would
    need devirtualizing those internal paths too.

## Generators (in /tmp, re-runnable from git HEAD)
/tmp/nm_generate.py (dispatch file), /tmp/nm_gen_registry.py (registry). Both read
pristine source via `git show HEAD:...` so re-running is idempotent.

## Phase 3 (submodule devirt) — DONE (commit 9e37185ce)
SUBMODULES static table → per-submodule statics + SUBMOD_REGISTRY; find_submodule via
registry; js_node_submod_install_<key>() emitted at all 6 codegen submodule-resolution
sites; black_box'd install-all hook for dynamic require/getBuiltinModule. hello-world
__text 3,971,424 → 3,716,980 (−254KB). CUMULATIVE baseline 4,667,824 → 3,716,980 =
−950,844 (−20.4%), ~5.4MB → ~4.3MB. 9/9 correctness sweep + fs/promises (named import
and fs.promises via native) byte-identical to node.

## console.trace — DONE (commit a5c14dbbf)
Coarse `at <anonymous>` frame instead of std::backtrace::force_capture (consistent with
Error.stack; prereq for any future panic-symbolizer strip). No size change alone (the
143KB gimli is pulled by std's panic runtime, not console.trace).

## Panic-symbolizer strip (~220KB) — ATTEMPTED, REVERTED (toolchain-fragile)
build-std + panic_immediate_abort to drop std's default panic hook + DWARF symbolizer.
Blockers found on current nightly:
  1. panic_immediate_abort is now a real STRATEGY: needs `-Cpanic=immediate-abort`
     (+ -Zunstable-options) + -Zbuild-std, NOT the old `-Zbuild-std-features=panic_immediate_abort`.
  2. Native build (host==target) → host build-scripts/proc-macros use the PRECOMPILED host
     core (default panic), but the rustflag forces immediate-abort on them → "core compiled
     with incompatible panic strategy" (proc-macro2 build script fails).
  3. Fix requires explicit `--target <host-triple>` to separate host (precompiled) from
     target (build-std immediate-abort) — which then breaks the auto-opt output-path
     resolution (libs move to target/<triple>/release/). 3 fragile, nightly-version-specific
     pieces → defer to a focused effort with proper --target + path handling.
The PERRY_MIN_SIZE=1 opt-in wiring was reverted (kept the tree clean). console.trace prereq
stays. Other no-tradeoff levers remain: json (47KB, event-loop pump), js_native_call_method
monolith (34KB devirt), feature-gating url/intl/bigint (160KB, other branch's mechanism).
