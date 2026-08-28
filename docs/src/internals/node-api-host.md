# Node-API host design

Status: implemented contract for [#8523](https://github.com/PerryTS/perry/issues/8523).
The implementation follows the completed `bun:ffi` callback work in #6562.
This document is the representation, lifetime, ABI, loader, and shipping
contract kept alongside the implementation and its gates.

## Implementation status

The optional `perry-runtime/node-api-host` feature contains the version-8 host:
opaque handle scopes and references, GC root rewriting and weak clearing,
object metadata/finalization, values and descriptors, native callbacks,
buffers/views, promises, async work, threadsafe functions, cleanup hooks, and
the authenticated module loader. The compiler enables it only after the graph
reaches a `.node` file owned by an exact host `perry.nativeAddons` entry.

Approved addons are screened for direct libuv/V8/NAN/Node C++ imports, copied
to the relocatable `<executable>.perry-native` sidecar, hashed into its manifest
and build cache, and loaded only after every payload hash is verified.
`require()` and `process.dlopen()` share that authorization and cache. Linker
exports come from the checked-in version-8 symbol inventory and are absent when
the addon graph is empty, preserving the zero-byte default path.

The integration gate compiles and executes a direct C addon, verifies its
imports resolve from the host executable, authenticates and then deliberately
corrupts its sidecar, and enforces both size bounds. Pinned published-package
gates compare `@napi-rs/snappy` 1.0.2 sync/async results with Node and compare
the real `@parcel/watcher` 2.5.1 `watcher.node` snapshot stream with Perry's
facade. CI treats missing npm tools or network for those differentials as a
failure; local offline runs explain their skip unless `PERRY_REQUIRE_NPM_E2E=1`
is set.

The host lets a Perry executable load a prebuilt Node-API (`.node`) addon
without embedding Node, V8, JavaScriptCore, or another JavaScript engine. It is
an opt-in compatibility route for the long tail of addons. A package with a
Perry facade remains on that facade: the host never supersedes a
`well_known_bindings.toml` entry.

## Decisions at a glance

| Area | Decision |
|---|---|
| Advertised API | Node-API version 8 |
| `napi_env` | One environment per Perry agent/realm, owned by its JavaScript thread |
| `napi_value` | Opaque host token containing an index and generation for an environment-local handle slot; never a Perry heap address |
| Handle roots | Open handle scopes are mutable GC roots and are rewritten after evacuation |
| `napi_ref` | Strong references root their value; zero-count references use Perry's existing weak-target machinery |
| Finalizers | Address-keyed object metadata is rekeyed on moves and death-pruned after marking; callbacks are queued and run on the owning main thread outside GC |
| Exceptions | Perry throws are trapped inside the host, stored in the environment, and returned as `napi_pending_exception`; no unwind crosses addon code |
| Off-thread API | Only TSFN call/acquire/release operations are legal; every other operation verifies the owning thread |
| Module entry | Prefer `napi_register_module_v1`; support constructor-time `napi_module_register` for legacy addons |
| Unsupported ABI | NAN, V8, direct `uv_*`, and mobile targets are rejected before initialization |
| Shipping | Relocatable sidecar directory beside the executable; no extract-at-first-run path |
| Policy | Exact package-name allowlist in `package.json` under `perry.nativeAddons` |
| Size gate | No addon in the graph means no host archive references, no exported Node-API symbols, and a zero-byte executable delta |

Version 8 is the baseline selected by Node's own v22 headers when an addon does
not request a newer version. It includes BigInt, dates, detach, type tags,
async cleanup, and the complete TSFN surface needed by current napi-rs and
node-addon-api packages, while avoiding a false claim for the version 9 and 10
extras. A module whose `node_api_module_get_api_version_v1()` returns more than
8 is rejected before its initializer runs, with the requested and supported
versions in the diagnostic.

## Environment and handle representation

`napi_env` points to a host-owned `Env` that is not a GC object. It contains:

- a unique environment id and the owning `ThreadId`;
- the open handle-scope stack and handle-slot slab;
- references, native callback records, deferred promises, async work, and
  threadsafe functions owned by the environment;
- a rooted pending-exception slot and stable `napi_extended_error_info`
  storage;
- instance data, environment cleanup hooks, the current module filename, and
  a `can_call_into_js` state;
- the environment lifecycle (`loading`, `running`, `closing`, or `closed`).

The environment is created for the Perry agent before the first addon loads
and destroyed after the event pump has stopped accepting work. Different
Perry agents never share an environment or a heap value.

### `napi_value`

A `napi_value` is an opaque pointer to a non-moving host token. The token holds
`(env_id, slot_index, generation)`. The indexed `HandleSlot` holds the Perry
NaN-box bits, its owning scope, its generation, and a live bit. Thus addon code
never observes a raw `ObjectHeader`, `StringHeader`, or other moving heap
address.

Tokens are stable for the lifetime of the environment and are not reused.
Slots may be reused only after their generation advances. Every entry point
validates all three token fields before reading a value. This makes an illegal
handle use after scope close return `napi_invalid_arg`, rather than aliasing a
new value or dereferencing reclaimed memory.

Each `ScopeFrame` records the slot indices created in that scope and whether an
escapable scope has already escaped a value. Closing a scope invalidates its
slots. `napi_escape_handle` copies the selected value into a fresh slot in the
parent scope and may succeed once. Module initialization and every native
callback get an implicit scope, so an addon is not required to open a scope
before creating ordinary return values.

All live scope slots are visited by `scan_node_api_roots_mut`, registered in
`gc_init()` with a descriptive name. The scanner uses mutable NaN-box visits,
so a copied minor or old-generation evacuation rewrites each slot in place.
The pending exception, strong references, callbacks, deferred resolutions, and
queued main-thread completions are visited by the same scanner. The new tables
must be classified in `scripts/gc_runtime_root_holders.json` in the commit that
introduces them.

The following invariant applies at every host call site:

> A Perry heap value is written into a live handle slot before any operation
> that can allocate or call JavaScript, and is reread from that slot after the
> operation.

In particular, the host never caches `HandleSlot.value_bits` in a Rust local
across property access, conversion, callback invocation, or allocation.

### References, including weak references

`napi_ref` is a stable host record, also tagged with its environment id and a
generation. A positive reference count stores its value in a strong slot
visited by `scan_node_api_roots_mut`.

Weak references are in version 1; deferring them would exclude common
node-addon-api and napi-rs patterns. A zero-count reference roots a hidden
Perry `WeakRef` holder, not the target. Perry already skips and rewrites that
holder's weak target slot and clears it during every collection. Consequently:

1. `napi_create_reference(..., 0, ...)` creates the hidden weak holder while
   the input handle is still rooted.
2. `napi_reference_ref` reads the weak target. If it has been collected it
   returns `napi_ok` with count zero without resurrecting it; otherwise the
   target moves into the strong slot before the weak holder is released.
3. `napi_reference_unref` changing `1 -> 0` creates a weak holder before
   clearing the strong slot.
4. `napi_get_reference_value` returns a null C pointer when a weak target has
   been collected, matching Node-API; that is distinct from a handle for the
   JavaScript value `null`.

Values that cannot be held weakly (for example, number primitives) remain
strongly retained even at refcount zero, matching Node's reference behavior.
Deleting a reference invalidates the record and releases either root.
Refcounts use checked arithmetic and return `napi_generic_failure` on overflow,
underflow, or an already-deleted record.

## Native-owned data and finalization

Wrap data, externals, type tags, and finalizer records live in a per-agent
`NODE_API_OBJECT_META` table keyed by the owning Perry user address. A table
entry contains native pointers and identifiers only; it never keeps its owner
alive.

The table has both halves required by a moving collector:

- `scan_node_api_object_meta_keys_mut` visits keys as metadata. It follows
  forwarding records and rekeys entries without marking the owner.
- `prune_dead_node_api_object_meta_owners` is registered in
  `gc::dead_owner::DEAD_KEY_PRUNES`. Both post-trace and copied-minor fan-out
  remove entries whose owners are proven dead.

The rekey site and death prune receive matching entries in
`scripts/gc_rekeyed_key_tables.json`. Merely registering a strong root scanner
would be incorrect here because it would make every wrapped object immortal.

Death pruning moves finalizer records into a native pending queue. It does not
call addon code while the collector is marking, rewriting, sweeping, or
holding an arena borrow. The next main-thread safepoint drains the queue inside
an implicit handle scope with `can_call_into_js = true`. That is the stable
version-8 `napi_finalize` contract and is why finalizer invocation depends on
the completed native-to-JavaScript callback boundary. The experimental
`node_api_basic_env` restriction and `node_api_post_finalizer` pairing are
deferred with the rest of that experimental surface.

`napi_wrap` installs at most one wrap record per object. `napi_unwrap` reads it.
`napi_remove_wrap` atomically removes it, returns the native pointer, and
prevents its finalizer from running. `napi_add_finalizer` may add multiple
independent records. When it returns a `napi_ref`, that record also identifies
the finalizer. Deleting that reference before collection removes the host's
tracking record, so the callback may never run; deleting it from inside the
already-queued callback only releases the reference.

External buffers and array buffers use the same finalizer queue. Environment
shutdown first prevents new work, drains TSFNs and async completions, runs
cleanup hooks, then enqueues and drains all remaining environment-owned
finalizers exactly once. A finalizer record has an atomic state
`registered -> queued -> running -> complete`, so explicit removal, collection,
and shutdown cannot double-call it.

No finalizer ordering is promised between different objects. Records attached
to one object are queued in registration order. Environment cleanup hooks use
Node's LIFO order; async cleanup hooks hold shutdown open until their removal
callback is invoked.

## Status codes and pending exceptions

Every exported entry point is a plain `extern "C"` boundary and uses one common
prologue/epilogue:

1. validate the environment, lifecycle, owner thread, input pointers, and
   handle generations without panicking;
2. reject JavaScript-capable work when `can_call_into_js` is false;
3. run every operation that may throw through `exception::js_call_catching`;
4. on a Perry throw, root the thrown value in `Env.pending_exception` and
   return `napi_pending_exception`;
5. write output parameters only on the statuses for which Node-API defines an
   output;
6. update stable per-environment extended-error storage before returning.

`napi_throw` and its error helpers only set the pending slot. They do not call
`js_throw` while the addon is on the stack. When a native callback returns to
the host trampoline, the trampoline ignores its return value if an exception
is pending, closes the callback scope, and then raises the rooted exception on
the Perry side of the C boundary. This guarantees that neither Perry's system
unwinder nor its `setjmp` transport crosses third-party frames.

`napi_is_exception_pending` is a pure query.
`napi_get_and_clear_last_exception` creates a handle for the rooted exception
before clearing the environment slot. With no pending exception it writes a
null C pointer. While an exception is pending, ordinary APIs return
`napi_pending_exception`; the exception query/clear and error-information
operations remain available.

The error-info message is owned by the environment and remains valid until the
next Node-API call on that environment. `engine_error_code` is zero and
`engine_reserved` is null. `napi_fatal_error` writes the length-bounded location
and message directly to stderr and aborts. `napi_fatal_exception` transfers the
error to Perry's uncaught-exception path on the main thread.

## Functions and native classes

`napi_create_function` allocates a Perry closure whose captured host record
contains the native callback pointer, addon data pointer, name, and environment
id. A shared rest-argument trampoline constructs a stack-local
`napi_callback_info` containing rooted argument handles, `this`, callback data,
and the current `new_target`. It calls the addon callback inside an implicit
handle scope and converts the returned token back to a Perry value before
closing that scope.

`napi_call_function` uses Perry's general callable dispatch, not a closure-only
shortcut, so proxies, bound functions, and compiled JavaScript functions keep
their normal semantics. `napi_new_instance` uses the general construction path
and arms Perry's new-target state for the duration of the call.

`napi_define_class` allocates a synthetic class id and registers its constructor
and prototype in the existing class-id chain. Instance methods/accessors are
defined on the prototype and `napi_static` descriptors on the constructor.
The constructor's callback record owns the class id, and the default instance
is allocated and stamped before the native constructor runs. If the constructor
returns an object, normal JavaScript constructor replacement rules apply.

The registration also populates the runtime's dynamic-parent/prototype tables,
so a compiled class may extend a native-defined constructor and a native class
may extend another native class. `napi_instanceof` delegates to Perry's normal
`instanceof` operation, including `Symbol.hasInstance`, rather than comparing
only the immediate class id. The Stage 1 gate must include native base-class
subclassing because this crosses the runtime's historically weak dynamic-extends
path.

Property descriptors preserve `napi_writable`, `napi_enumerable`, and
`napi_configurable`. Getter and setter records use the same native callback
trampoline. A descriptor specifying incompatible `value`/`method`/accessor
fields is rejected before any property is changed.

## Buffers, views, and external memory

Perry `Buffer`, typed-array, array-buffer, and data-view byte storage is born
tenured and does not move. A returned native data pointer therefore remains
stable for the lifetime required by Node-API, while the wrapper itself remains
an ordinary rooted handle.

The host reuses the existing buffer/view registries for type identity, backing
array-buffer identity, offsets, and detach propagation. Creating an external
array buffer or buffer creates a zero-copy wrapper over the supplied bytes and
attaches its callback to `NODE_API_OBJECT_META`. A detached array buffer reports
zero length and null data, and all existing views observe the detach. Buffer
APIs reject detached storage where Node does.

`napi_adjust_external_memory` updates a signed per-environment counter and the
collector's external-memory pressure accounting. It neither allocates an
equivalent Perry buffer nor silently ignores the request. Underflow clamps at
zero for pressure accounting while the API's returned cumulative value remains
the checked signed total.

## Async work and threadsafe functions

`napi_create_async_work` creates an explicit state machine:

```text
created -> queued -> running -> completing -> complete -> deleted
                 \-> cancelled -> completing
```

Queueing uses `perry_ffi::spawn_blocking`. The execute callback runs on a worker
without access to Perry heap state. Completion is posted through the existing
main-thread event pump and runs inside an implicit scope. Cancellation succeeds
only before execution claims the work; completion still runs with
`napi_cancelled`. Deletion before completion marks the public handle deleted
but retains the internal record until neither worker nor completion owns it.

A TSFN owns a strong function reference (when a function is supplied), its
context, bounded or unbounded queue, thread count, ref/unref state, and finalizer.
Foreign threads may only call, acquire, or release it. They never touch a
`napi_value` or the Perry collector. `napi_call_threadsafe_function` copies the
opaque data pointer into the queue and notifies the main thread. A blocking call
waits for capacity; a blocking call from the owner thread with a full bounded
queue returns `napi_would_deadlock`.

The main-thread drain invokes `call_js_cb`, which may then use the environment
and supplied JavaScript callback. Abort release drains remaining items by
calling `call_js_cb` with null environment/function as required by Node-API.
The TSFN finalizer runs after the queue is empty and the thread count reaches
zero. A ref'd TSFN contributes to Perry's event-loop keepalive count; unref
removes that contribution without destroying the function.

`napi_async_init`, `napi_make_callback`, and `napi_async_destroy` bridge to the
existing `async_hooks` resource/context machinery. Callback scopes are a
validated nesting counter around that context; mismatch returns
`napi_callback_scope_mismatch`.

## Threading rules

The following calls are legal from a foreign thread:

- `napi_call_threadsafe_function`
- `napi_acquire_threadsafe_function`
- `napi_release_threadsafe_function`
- `napi_fatal_error` (it does not return)

All other entry points require the environment's owner thread, including
`napi_get_threadsafe_function_context`, TSFN ref/unref, reference operations,
and cleanup-hook registration. Entry points without an explicit `napi_env`
recover the owner from their validated opaque record. Misuse returns
`napi_generic_failure`, records a diagnostic when an environment is available,
and never reads or writes Perry heap state.

The execute half of async work is a foreign thread under this rule. The
complete half, module initialization, cleanup hooks, finalizers, native
callbacks, and TSFN `call_js_cb` all run on the owner thread.

## Module loading

The compile graph records every approved `.node` file as a native-addon module
instead of trying to read it as UTF-8. At runtime, one loader operation does the
following:

1. canonicalize the manifest-selected sidecar path beneath the executable's
   sidecar root;
2. return the cached exports object if that canonical file is already loaded;
3. set an environment-local `currently_loading` guard and load with
   `RTLD_NOW | RTLD_LOCAL` on Unix or safe `LoadLibraryExW` search flags on
   Windows;
4. reject an unresolved `uv_*`, V8, NAN, or non-Node-API Node symbol with the
   exact symbol and addon path in the error;
5. if present, call `node_api_module_get_api_version_v1` and reject versions
   above 8;
6. prefer `napi_register_module_v1(env, exports)`; otherwise use the descriptor
   captured by `napi_module_register` while the library constructor ran;
7. use the initializer's returned object, or the supplied exports object when
   the initializer returns null without an exception;
8. cache the rooted exports and library handle together.

`napi_module_register` outside an active load is an error. Multiple descriptors
from one library are rejected. An initializer exception closes the library,
discards the half-built cache entry, and propagates the rooted exception only
after control has returned from addon code.

Static `require()` of an approved addon lowers directly to this loader.
`process.dlopen(module, filename[, flags])` calls the same operation and writes
the resulting exports onto the supplied CommonJS module object. Unsupported
flags are rejected rather than ignored. Runtime-computed paths may load only a
file present in the compile-time addon manifest; the allowlist is not a general
`dlopen` capability.

The environment stores the active canonical module filename during
initialization. It is the future source for the version 9
`node_api_get_module_file_name` API, even though the version 8 host does not
export that symbol.

## Linking and exported symbols

The Node-API implementation lives behind a `node-api-host` runtime feature.
The compiler enables it only when the collected graph contains an approved
addon. A checked-in symbol inventory is the single source for Rust export
retention, platform linker flags, unresolved-symbol validation, and the CI
assertion.

- macOS removes `-Wl,-no_exported_symbols` only for an addon build and supplies
  an exported-symbols list containing the approved `_napi_*` names.
- Linux uses one `--export-dynamic-symbol=<name>` entry per approved symbol,
  not broad `--export-dynamic`.
- Windows supplies `/EXPORT:<name>` entries. The addon's standard delay-load
  hook resolves its `node.exe` imports against the current executable.

The loader itself uses the existing `bun_ffi` platform abstraction, extended
to report unresolved imports. Host symbols are retained and exported only when
the compile manifest is non-empty. A hello-world build therefore has exactly
the previous link command and runtime feature set.

## Opt-in and route precedence

The only opt-in is an exact package-name list in the project manifest:

```json
{
  "perry": {
    "nativeAddons": ["@swc/core", "oxc-parser"]
  }
}
```

Transitive packages cannot opt themselves in. The nearest project manifest is
authoritative, duplicate names are normalized, and subpaths inherit their
owning package's decision. Wildcards are not accepted.

Resolution order is:

1. a `well_known_bindings.toml` facade;
2. a package's explicit `perry.nativeLibrary`;
3. a project `perry.nativeAddons` entry;
4. ordinary JS/TS compilation;
5. the existing actionable unsupported-addon error.

Thus listing `better-sqlite3`, `sharp`, or `@parcel/watcher` does not bypass its
Perry facade. Node-API is faithful native execution, so an approved addon is
compatible with `PERRY_REQUIRE_FAITHFUL_BINDINGS=1`; partial hand-written
facade policy remains unchanged. NAN/V8 or direct-libuv imports remain hard
errors even when the package name is allowlisted.

Desktop/server targets are macOS, Windows, Linux, and the BSDs supported by
Perry's dynamic loader. iOS, tvOS, watchOS, visionOS, Android, HarmonyOS, and
WebAssembly reject `perry.nativeAddons` during target validation.

## Sidecar distribution

Addon builds emit a relocatable directory beside the executable:

```text
app
app.perry-native/
  manifest.json
  <package-name-hash>/
    watcher.node
    ...package-local shared libraries...
```

The compiler copies the selected platform package payload, preserving its
relative layout so `$ORIGIN`/`@loader_path` dependencies keep working. The
manifest records package name and version, target tuple, relative entry path,
SHA-256 for every copied file, and the Node-API policy version. Runtime loading
never consults the build machine's `node_modules` tree.

This sidecar model has no first-run write, so read-only install directories are
supported. Moving the executable requires moving its `.perry-native` sibling.
Missing or hash-mismatched files fail before `dlopen` with a packaging error.
The loader also requires the canonical entry path passed to the dynamic loader
to equal the canonical path of a payload file whose size and hash were just
verified. An existing file that is selected only by `entry`, but omitted from
`files`, is rejected.

The platform dynamic-loader APIs reopen that canonical pathname; they do not
provide one portable way to load from the file handle used for verification.
There is consequently an accepted time-of-check/time-of-use window between the
last payload hash and `dlopen`/`LoadLibraryExW`. Exploiting it requires write
access to the installed sidecar directory, which is outside this loader's
integrity boundary. Deployments must protect the executable and its sidecar
with the same ownership and write permissions (and platform signing where
applicable). The manifest checks detect packaging damage and tampering before
the first load; they are not a defense against a concurrent local writer.

For a macOS app bundle the directory is placed under `Contents/Frameworks`.
Every Mach-O sidecar and nested dylib is signed before the outer executable or
bundle is signed; notarization submits the complete bundle. For a loose command
line executable, release tooling signs each sidecar before the executable and
ships them in one archive. Perry does not strip quarantine attributes from
untrusted downloaded binaries at runtime.

Cross-compilation selects the target's platform package, never the host's.
Perry does not perform an unpinned registry download during compilation. The
target package must be materialized by the package manager/lockfile install;
when it is absent the diagnostic names the exact target tuple and candidate
optional package. This keeps registry credentials and dependency resolution in
the package manager while preventing a host `.node` file from entering a target
artifact.

## Cache identity

The compile-time addon manifest is sorted deterministically and contributes
the following to the build and link cache identities:

- policy schema version and advertised Node-API version;
- target tuple and normalized allowlist;
- selected package name/version and relative entry path;
- SHA-256 and size of every sidecar payload file;
- ordered exported-symbol inventory;
- shipping model (`sidecar-v1`).

The entry module's object-cache key also includes the canonical logical addon
ids it can load, because those ids appear in generated loader calls. Absolute
build paths do not enter any key or generated object. A sidecar hash change
must miss the top-level build cache even when all TypeScript and object files
are unchanged.

## Node-API surface inventory

The inventory is pinned to Node v26.5.1's
[`js_native_api.h`](https://github.com/nodejs/node/blob/v26.5.1/src/js_native_api.h)
and [`node_api.h`](https://github.com/nodejs/node/blob/v26.5.1/src/node_api.h).
`v1` means required before the host is usable. `later` means the declaration is
newer than the advertised version or experimental and is not exported.
`never` means Perry exports the version-8 symbol when necessary for binary
resolution, but it deterministically reports the stated unsupported facility.

### `js_native_api.h`: core through version 4

| Status | Entry points | Notes |
|---|---|---|
| v1 | `napi_get_last_error_info` | Stable per-environment storage |
| v1 | `napi_get_undefined`, `napi_get_null`, `napi_get_global`, `napi_get_boolean` | Singleton values receive ordinary scoped handles |
| v1 | `napi_create_object`, `napi_create_array`, `napi_create_array_with_length` | Perry object/array allocators |
| v1 | `napi_create_double`, `napi_create_int32`, `napi_create_uint32`, `napi_create_int64` | Perry NaN-box conversions |
| v1 | `napi_create_string_latin1`, `napi_create_string_utf8`, `napi_create_string_utf16` | Length-bounded; `NAPI_AUTO_LENGTH` supported |
| v1 | `napi_create_symbol`, `napi_create_function` | Native callbacks use host records |
| v1 | `napi_create_error`, `napi_create_type_error`, `napi_create_range_error` | `code` is installed when supplied |
| v1 | `napi_typeof` | Includes function, external, symbol, and bigint distinctions |
| v1 | `napi_get_value_double`, `napi_get_value_int32`, `napi_get_value_uint32`, `napi_get_value_int64`, `napi_get_value_bool` | Checked type/status behavior |
| v1 | `napi_get_value_string_latin1`, `napi_get_value_string_utf8`, `napi_get_value_string_utf16` | Query-length and NUL-termination semantics included |
| v1 | `napi_coerce_to_bool`, `napi_coerce_to_number`, `napi_coerce_to_object`, `napi_coerce_to_string` | User code is exception-trapped |
| v1 | `napi_get_prototype`, `napi_get_property_names` | General object semantics |
| v1 | `napi_set_property`, `napi_has_property`, `napi_get_property`, `napi_delete_property`, `napi_has_own_property` | String, symbol, and numeric keys |
| v1 | `napi_set_named_property`, `napi_has_named_property`, `napi_get_named_property` | UTF-8 names |
| v1 | `napi_set_element`, `napi_has_element`, `napi_get_element`, `napi_delete_element` | Arrays and exotic indexed objects |
| v1 | `napi_define_properties` | Data, method, and accessor descriptors |
| v1 | `napi_is_array`, `napi_get_array_length`, `napi_strict_equals` | No pointer-identity shortcut |
| v1 | `napi_call_function`, `napi_new_instance`, `napi_instanceof` | General Perry dispatch/construction |
| v1 | `napi_get_cb_info`, `napi_get_new_target`, `napi_define_class` | Callback-info lifetime is the native call |
| v1 | `napi_wrap`, `napi_unwrap`, `napi_remove_wrap`, `napi_create_external`, `napi_get_value_external` | Native object metadata table |
| v1 | `napi_create_reference`, `napi_delete_reference`, `napi_reference_ref`, `napi_reference_unref`, `napi_get_reference_value` | Strong and weak reference design above |
| v1 | `napi_open_handle_scope`, `napi_close_handle_scope`, `napi_open_escapable_handle_scope`, `napi_close_escapable_handle_scope`, `napi_escape_handle` | Strict nesting and generation validation |
| v1 | `napi_throw`, `napi_throw_error`, `napi_throw_type_error`, `napi_throw_range_error`, `napi_is_error` | Pending-slot model |
| v1 | `napi_is_exception_pending`, `napi_get_and_clear_last_exception` | Available while pending |
| v1 | `napi_is_arraybuffer`, `napi_create_arraybuffer`, `napi_create_external_arraybuffer`, `napi_get_arraybuffer_info` | Stable backing pointers |
| v1 | `napi_is_typedarray`, `napi_create_typedarray`, `napi_get_typedarray_info` | All eleven declared typed-array kinds |
| v1 | `napi_create_dataview`, `napi_is_dataview`, `napi_get_dataview_info` | Backing identity and offsets preserved |
| v1 | `napi_get_version` | Returns 8 |
| v1 | `napi_create_promise`, `napi_resolve_deferred`, `napi_reject_deferred`, `napi_is_promise` | Deferred records are environment-owned roots |
| never | `napi_run_script` | Returns `napi_generic_failure`; arbitrary runtime source execution would violate the no-runtime-engine model |
| v1 | `napi_adjust_external_memory` | Collector pressure accounting |

### `js_native_api.h`: versions 5 through 8

| Status | Version | Entry points | Notes |
|---|---:|---|---|
| v1 | 5 | `napi_create_date`, `napi_is_date`, `napi_get_date_value` | Perry `Date` identity/value |
| v1 | 5 | `napi_add_finalizer` | Queued post-GC finalization |
| v1 | 6 | `napi_create_bigint_int64`, `napi_create_bigint_uint64`, `napi_create_bigint_words` | Arbitrary precision |
| v1 | 6 | `napi_get_value_bigint_int64`, `napi_get_value_bigint_uint64`, `napi_get_value_bigint_words` | Exact `lossless` and word-count behavior |
| v1 | 6 | `napi_get_all_property_names` | Collection mode, filter, and key conversion honored |
| v1 | 6 | `napi_set_instance_data`, `napi_get_instance_data` | One record per environment; replacement overwrites without calling the previous finalizer |
| v1 | 7 | `napi_detach_arraybuffer`, `napi_is_detached_arraybuffer` | Existing detach propagation |
| v1 | 8 | `napi_type_tag_object`, `napi_check_object_type_tag` | 128-bit tag in object metadata |
| v1 | 8 | `napi_object_freeze`, `napi_object_seal` | Perry descriptor machinery |

### `js_native_api.h`: version 9, version 10, and experimental

| Status | Version | Entry points | Reason |
|---|---:|---|---|
| later | 9 | `node_api_symbol_for`, `node_api_create_syntax_error`, `node_api_throw_syntax_error` | Advertise only with a complete version-9 surface |
| later | 10 | `node_api_create_external_string_latin1`, `node_api_create_external_string_utf16` | Requires external string lifetime/accounting work |
| later | 10 | `node_api_create_property_key_latin1`, `node_api_create_property_key_utf8`, `node_api_create_property_key_utf16` | Version-10 fast-path aliases |
| later | experimental | `node_api_post_finalizer` | Not part of the advertised stable ABI |
| later | experimental | `node_api_create_object_with_properties`, `node_api_set_prototype` | Not part of the advertised stable ABI |
| later | experimental | `node_api_create_sharedarraybuffer`, `node_api_create_external_sharedarraybuffer`, `node_api_is_sharedarraybuffer` | Not part of the advertised stable ABI |

### `node_api.h`

| Status | Version | Entry points | Notes |
|---|---:|---|---|
| v1 | base | `napi_module_register`, `napi_fatal_error` | Legacy registration and abort path |
| v1 | base | `napi_async_init`, `napi_async_destroy`, `napi_make_callback` | Existing async-hooks integration |
| v1 | base | `napi_create_buffer`, `napi_create_external_buffer`, `napi_create_buffer_copy`, `napi_is_buffer`, `napi_get_buffer_info` | Perry `Buffer` identity and stable bytes |
| v1 | base | `napi_create_async_work`, `napi_delete_async_work`, `napi_queue_async_work`, `napi_cancel_async_work` | Explicit state machine above |
| v1 | base | `napi_get_node_version` | Returns Perry's semver components with release string `perry`; it does not claim a Node release |
| never | 2 | `napi_get_uv_event_loop` | Returns `napi_generic_failure` and a null loop; Perry has no libuv |
| v1 | 3 | `napi_fatal_exception`, `napi_add_env_cleanup_hook`, `napi_remove_env_cleanup_hook` | Main-thread lifecycle |
| v1 | 3 | `napi_open_callback_scope`, `napi_close_callback_scope` | Async context and strict nesting |
| v1 | 4 | `napi_create_threadsafe_function`, `napi_get_threadsafe_function_context`, `napi_call_threadsafe_function` | Event-pump-backed TSFN |
| v1 | 4 | `napi_acquire_threadsafe_function`, `napi_release_threadsafe_function`, `napi_unref_threadsafe_function`, `napi_ref_threadsafe_function` | Thread count and event-loop keepalive |
| v1 | 8 | `napi_add_async_cleanup_hook`, `napi_remove_async_cleanup_hook` | Shutdown waits for completion |
| later | 9 | `node_api_get_module_file_name` | Environment already records the future value |
| later | 10 | `node_api_create_buffer_from_arraybuffer` | Advertise with version 10 |

The addon-side initializer exports
`node_api_module_get_api_version_v1` and `napi_register_module_v1`; these are
looked up in the addon and are not host exports.

## Required gates

Implementation is not complete until the gates below can independently fail:

1. **Handle/GC gate:** scopes, strong and weak refs, wrap metadata, callbacks,
   and pending exceptions survive forced copied minors and full collections;
   stale handles fail generation validation; the root-holder and rekey-table
   audit scripts are clean.
2. **Exception gate:** a throwing getter, native callback, finalizer misuse,
   and TSFN callback all return through C before Perry raises; an instrumented
   addon asserts that no unwind entered its frames.
3. **Loader/export gate:** a real addon records the address of a called
   `napi_*` function and the test proves that address belongs to the Perry
   executable. The smoke call count must be greater than zero.
4. **Real-addon gate:** one node-addon-api addon and one napi-rs addon cover
   properties, classes, async work, TSFN, instance data, wrap/finalizers, and
   buffers. Unsupported NAPI version, NAN/V8, and `uv_*` fixtures assert their
   exact diagnostics.
5. **Watcher differential gate:** the real `@parcel/watcher` binary and Perry's
   facade observe the same fixture tree and emit identical coalesced streams;
   both sides assert that their native implementation actually ran.
6. **Distribution gate:** move the executable plus sidecar directory to a
   read-only location and load successfully; deletion or mutation of a
   sidecar fails the manifest hash check.
7. **Cache gate:** changing only addon bytes or policy misses the build cache;
   rebuilding unchanged inputs hits it.
8. **Size gate:** byte-identical hello-world outputs with an empty addon graph;
   report the host-only delta for an addon build and enforce the 0.6 MB budget.

The host is not enabled merely because its unit tests pass. The real-addon and
symbol-provenance gates are the acceptance boundary: a green run in which no
addon initializer or `napi_*` body executed is a failure.
