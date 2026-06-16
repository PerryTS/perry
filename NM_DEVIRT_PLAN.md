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
[ ] FOLLOW-UPS:
    - install_all NOT yet emitted for truly-dynamic `import(runtimeVar)` of a native module → that path's method dispatch would return undefined. Wire js_nm_install_all() at the dynamic-import codegen site (the await-import-of-literal path IS covered). Rare; static imports + global process all work.
    - node_v8/perf_hooks/cluster internal creators: covered because reachable only when their module is imported (→ codegen install ran). Verify with a v8-serializer / perf_hooks test.
[ ] constructor-dispatcher (js_new_function_construct, class_registry.rs) — phase 2 (residual child_process/cluster/readline syms still pinned by it; another chunk of __text).

## Generators (in /tmp, re-runnable from git HEAD)
/tmp/nm_generate.py (dispatch file), /tmp/nm_gen_registry.py (registry). Both read
pristine source via `git show HEAD:...` so re-running is idempotent.
