# Native bindings — overview

Perry compiles TypeScript to native executables. When user code says
`import { createConnection } from "mysql2"`, the call doesn't bottom out
in JavaScript-engine glue — it lands on a Rust function that's been
linked into the binary as `extern "C"`. This page is the map of how
that works end-to-end.

Native bindings are an escape hatch for native and system boundaries, not
Perry's default npm compatibility mechanism. Pure JavaScript and TypeScript
packages should be compiled from their upstream source, and missing shared
Node.js or Web APIs should be implemented once in the runtime. The in-tree
well-known table still contains compatibility shims while those migrations are
completed; see [Bundled native-binding governance](governance.md) for the
policy and the decision recorded for every extension crate.

## The big picture

There are four layers, from most stable to most flexible:

```text
┌─────────────────────────────────────────────────────────────────┐
│  Layer 4: User TypeScript                                        │
│    import { createConnection } from "mysql2";                     │
│    const c = await createConnection({ host, user, password });    │
│    const [rows] = await c.query("SELECT 1");                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ resolved at compile time → maps to
                              │ js_mysql2_* extern "C" symbols
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Layer 3: Bindings packages                                      │
│    Three sources, queried in this order:                          │
│                                                                   │
│    a. node_modules/<name>/ with perry.nativeLibrary               │
│       → the user installed an external binding via                │
│         `bun add @scope/<name>`. Wins over (b) and (c).           │
│                                                                   │
│    b. node_modules/<name>/ selected by compilePackages            │
│       → compile the installed JavaScript/TypeScript source.        │
│                                                                   │
│    c. well-known table (well_known_bindings.toml)                 │
│       → Perry ships a runtime API or transitional compatibility  │
│         shim. The governance inventory records its destination.  │
│                                                                   │
│    d. nothing matches → resolution error at compile time.         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ all wrapper crates depend on this:
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Layer 2: perry-ffi crate (the stable ABI)                       │
│    pub fn alloc_string(s: &str) -> JsString                       │
│    pub fn read_string(JsString) -> Option<&'static str>           │
│    pub struct JsValue(u64); JsPromise; JsClosure; ...             │
│                                                                   │
│    9 surface dimensions: strings, async/Promise, handle           │
│    registry, JsValue/objects/arrays, binary bytes, closures,     │
│    GC root scanner, BigInt, Buffer, JSON-stringify, event-pump.  │
│                                                                   │
│    Wrapper authors depend ONLY on perry-ffi. perry-runtime's     │
│    internals (NaN-box tags, struct layouts) can change between    │
│    releases without breaking wrappers.                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ implementation detail of:
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Layer 1: perry-runtime / perry-stdlib internals                 │
│    StringHeader / ArrayHeader / ObjectHeader layouts, NaN-      │
│    boxing tags, generational GC, arena allocator, async runtime,│
│    the 30+ in-tree native modules (perry/ui, perry/thread, ...).│
│    Free to change between Perry releases — the perry-ffi semver  │
│    is the only stable contract.                                  │
└─────────────────────────────────────────────────────────────────┘
```

The whole point: **anyone can publish a binding**. A third-party crate
ships an npm package containing a Rust crate, a `package.json` with a
`perry.nativeLibrary` block, and prebuilt staticlibs. Users
`bun add` it. Perry's compiler picks it up automatically. No PR to the
Perry repo, no central registry approval, no `@perryts/` namespace
required.

## Worked example: `import { createConnection } from "mysql2"`

Step by step, what happens when you `perry compile` a program with
that import:

### 1. Module resolution

Perry's resolver
([`crates/perry/src/commands/compile/resolve.rs`](https://github.com/PerryTS/perry/blob/main/crates/perry/src/commands/compile/resolve.rs))
walks each search path looking for `node_modules/mysql2/`:

- **If `node_modules/mysql2/package.json` exists with a
  `perry.nativeLibrary` block**: parse the manifest, treat the package
  as a native binding. Skip layers (c) and (d).
- **If `mysql2` is selected through `perry.compilePackages`**: compile the
  installed JavaScript package source. This is the intended long-term path for
  `mysql2` and other source packages.
- **Otherwise**: consult the
  **well-known table** at
  [`crates/perry/well_known_bindings.toml`](https://github.com/PerryTS/perry/blob/main/crates/perry/well_known_bindings.toml).
  The table maps `mysql2` → `perry-ext-mysql2` (a Rust crate that
  ships in the Perry install). The user didn't `npm install` anything;
  Perry handles it.
- **If nothing matches**: compile error pointing at the import line.

### 2. ABI version check

If the resolved binding has a `perry.nativeLibrary.abiVersion` field
(required from v0.6.0 onwards; warning-only in v0.5.x), Perry verifies
the declared semver range covers the bundled `perry-ffi` version. A
binding declaring `"0.5"` loads under any `0.5.x` Perry; one declaring
`"^1.0"` loads only under `1.x`. Mismatches are a hard compile error
with a recipe pointing at the offending package.

See [`manifest-v1.md`](manifest-v1.md) for the full schema.

### 3. Symbol mapping

The manifest's `functions[]` block lists every `extern "C"` symbol
the staticlib exports plus their TypeScript-visible signature:

```json
{
  "functions": [
    {
      "name": "js_mysql2_create_connection",
      "params": ["jsvalue"],
      "returns": "promise"
    },
    {
      "name": "js_mysql2_connection_query",
      "params": ["i64", "string", "jsvalue"],
      "returns": "promise"
    }
  ]
}
```

Perry's codegen translates the user's TS-side calls
(`mysql.createConnection(config)`, `c.query(sql, params)`) into direct
calls to these symbols, with the right argument coercion (JsValue
NaN-box ↔ f64 ABI shim, string-pointer extraction, etc.).

Those manifest entries are native ABI descriptors, not TypeScript
types. A descriptor like `f32` or `usize` chooses the native slot used
for the C call and still appears to user code as a JavaScript
`number`; `buffer+len` consumes one Buffer/Uint8Array-shaped argument
and emits `(ptr, usize)` native slots; `handle<T>` and `promise<T>`
carry metadata while remaining opaque runtime handles at the boundary.
See [`manifest-v1.md`](manifest-v1.md) for the full descriptor
vocabulary and the legacy string aliases that remain accepted.

### 4. Linking

The staticlib (`libperry_ext_mysql2.a` for the well-known case, or a
prebuilt artifact in `node_modules/mysql2/prebuilt/<target>/` for the
external case) joins the link line alongside `libperry_runtime.a` and
`libperry_stdlib.a`. The `js_mysql2_*` symbol references in the
user's compiled code resolve at link time.

If the binding ships only Rust source (no prebuilt), Perry runs
`cargo build --release` on the wrapper at compile time. Slow first
build, then cached.

### 5. Runtime

User code runs. Calls into `js_mysql2_*` happen at native speed —
function call overhead is one register-pass for the receiver handle
plus one each per param. Promise resolution / closure invocation /
async work bridge through perry-ffi's surface (`JsPromise`,
`JsClosure`, `spawn_blocking + tokio::Handle::current().block_on`).
The wrapper sees Perry's NaN-boxed JsValues directly; user TypeScript
sees a normal Promise / object / array.

## What perry-ffi guarantees

The 9 surface dimensions perry-ffi exposes today are:

| Surface | What it does | Documented at |
|---|---|---|
| Strings | `JsString` / `alloc_string` / `read_string` / `read_bytes` / `alloc_bytes` | [`abi.md`](abi.md) |
| Async / Promise | `JsPromise` (`new` / `resolve` / `reject_string` as `Error` / `reject_with`), `spawn_blocking` | [`abi.md`](abi.md) |
| Handles | `register_handle` / `get_handle` / `with_handle` / `take_handle` / `iter_handles_of` | [`abi.md`](abi.md) |
| JsValue + objects/arrays | `JsValue`, `js_array_alloc/push/get/set`, `js_object_alloc_with_shape`, `js_object_get_field`, `js_object_set_field`, `build_object_shape` | [`abi.md`](abi.md) |
| Closures | `JsClosure::call0..4` | [`abi.md`](abi.md) |
| GC root scanner | `gc_register_root_scanner` | [`abi.md`](abi.md) |
| BigInt | `BigIntHeader`, `alloc_bigint_from_str`, `read_bigint_limbs` | [`abi.md`](abi.md) |
| Buffer | `BufferHeader`, `alloc_buffer`, `read_buffer_bytes` | [`abi.md`](abi.md) |
| JSON-stringify | `json_stringify(JsValue) -> Option<String>` | [`abi.md`](abi.md) |
| Event pump | `notify_main_thread` | [`abi.md`](abi.md) |

A wrapper that uses anything outside this list (e.g. reaches into
`perry_runtime::*` types directly) is **off-contract** — its build
will break the next time those types change. Stay on perry-ffi.

The [`abi.md`](abi.md) page is the source of truth for what's in each
surface. The semver promise: **breaking changes to anything documented
there bump perry-ffi major**, regardless of what `perry-runtime` does
internally.

## Code organization

```text
crates/
  perry-ffi/              ← Layer 2: the stable ABI surface
  perry-runtime/          ← Layer 1: NaN-boxing, GC, arena, JS objects
  perry-stdlib/           ← Layer 1: in-tree wrappers (perry/ui, fs,
                            crypto helpers, etc. — anything genuinely
                            coupled to runtime internals)
  perry-ext-<name>/       ← Layer 3: selectively linked runtime APIs and
                            compatibility shims. Their retained/migration
                            decisions live in workspace-architecture.json.

External native bindings (Layer 3, third-party — Rust + perry-ffi):
  PerryTS/tursodb-bindings    → bun add @perryts/tursodb
  PerryTS/iroh-bindings       → bun add @perryts/iroh
  <anyone>/whatever-bindings  → user publishes themselves

External pure-TypeScript drivers (compiled via compilePackages):
  PerryTS/postgres            → bun add @perryts/postgres
  PerryTS/mysql               → bun add @perryts/mysql
  PerryTS/mongodb             → bun add @perryts/mongodb
  PerryTS/redis               → bun add @perryts/redis
```

The split between **well-known** in-tree wrappers and **external** is a
packaging convention, not a technical distinction. Both depend only on
perry-ffi; both ship `extern "C"` symbols Perry's codegen calls. The policy
difference is intentional: an in-tree wrapper needs a shared runtime reason to
remain in core. Ordinary source packages should migrate to
`compilePackages`, while domain-specific native integrations should migrate to
external packages with independent releases.

The two existing external native wrappers (`tursodb`, `iroh`) cover
functionality that doesn't have an in-tree perry-stdlib equivalent —
they're net-new bindings that originated as third-party packages.
That validates the contract: perry-ffi is sufficient to write a real
wrapper without forking Perry.

## Three paths to a database driver (postgres / mysql / mongodb / redis)

Perry currently ships parallel database-driver families. The bundled native
drivers remain compatibility bridges; compiling real JavaScript/TypeScript
drivers is the migration target:

| Path | Install | Resolver layer | What it is |
|---|---|---|---|
| **Well-known native binding** | nothing (bundled) | (c) | Compatibility path: `import 'mysql2'` / `import 'pg'` / `import 'mongodb'` route to in-tree Rust wrappers. They remain available until the source-package migration gates pass. |
| **`@perryts/{postgres,mysql,mongodb,redis}`** | `bun add @perryts/postgres` | (a) | Pure-TypeScript wire-protocol drivers — no Rust, no native dep. Use Perry's [`compilePackages`](../packages/porting.md) to compile the TS to native via LLVM. Also run unmodified on Node.js / Bun. Independent semver. |
| **External native binding** | `bun add @perryts/tursodb` | (a) | Third-party Rust crate using `perry-ffi`, manifest at `package.json::perry.nativeLibrary`. Today: `@perryts/tursodb`, `@perryts/iroh`. |

Resolution precedence (per layer (a) → (b) → (c) above): an installed
`@perryts/mysql` does **not** override `import 'mysql2'` because the
package names are different. If you `bun add @perryts/mysql` and also
`import 'mysql2'` in the same program, both drivers ship in the
binary — they're independent. To opt out of the well-known `mysql2`
shim, just don't import `mysql2`.

**When to pick which:**

- **Well-known native (`mysql2` / `pg` / `mongodb`)** — current zero-install
  compatibility path; its feature set tracks Perry's release cadence and it is
  scheduled to yield to compiled package source.
- **`@perryts/postgres` / `@perryts/mysql` / `@perryts/mongodb` / `@perryts/redis`** —
  you want to read / fork / patch the driver in plain TypeScript;
  you want the same code running on Node.js or Bun for fallback;
  you need a feature ahead of Perry's next release.
- **External native binding** — you're wrapping a Rust crate that
  doesn't have a JS-only equivalent (Tursodb's embedded SQLite-
  compatible engine, Iroh's QUIC transport).

## Concrete how-tos

| If you want to … | Read |
|---|---|
| **Use a currently bundled compatibility shim** | `import` it directly, or explicitly select the installed package in `perry.compilePackages` to exercise the preferred source path. |
| **Use a third-party native binding** | `bun add <package>`, then `import`. Perry's resolver finds it via `node_modules/<pkg>/package.json`. |
| **Find which packages ship out-of-the-box** | `perry native list` |
| **Write your own native binding** | `perry native init my-bindings` scaffolds the Cargo crate + `package.json` + `release.yml` for prebuilds. Then read [`abi.md`](abi.md) for the perry-ffi surface and [`manifest-v1.md`](manifest-v1.md) for the manifest schema. |
| **Verify your binding's manifest matches its `.a`** | `cd my-bindings && perry native validate` (runs `cargo build --release`, walks `nm -gP` over the staticlib, diffs against `functions[]`, reports missing or undeclared symbols). |
| **Override a well-known binding** | Install your fork into `node_modules/<name>/` with a `perry.nativeLibrary` block. Resolution layer (a) wins over layer (c). |
| **See what stdlib APIs Perry implements** | Auto-generated from the manifest: [`docs/src/api/reference.md`](../api/reference.md). The `perry types` command writes a current snapshot to `.perry/types/stdlib/index.d.ts` for editor squiggles. |

## Authoring a binding — the 60-second tour

```sh
# Scaffold
perry native init my-pdf --description "PDF rendering bindings" \
  --upstream-dep 'pdfium-render = "0.8"'
cd my-pdf

# Edit src/lib.rs — add your `js_*` functions, all using only
# `perry_ffi::*` types
$EDITOR src/lib.rs

# Edit src/index.ts — declare the TS surface user code imports
$EDITOR src/index.ts

# Edit package.json — list every js_* export in the
# perry.nativeLibrary.functions[] block
$EDITOR package.json

# Verify
perry native validate
# ✅ manifest matches the staticlib

# Publish
git tag v0.1.0 && git push --tags  # the scaffolded release.yml
                                    # builds prebuilts for all targets
                                    # and attaches them to the release
npm publish
```

A user can now `bun add my-pdf` and `import { renderPdf } from "my-pdf"`
in their Perry program.

## Versioning policy

- **`perry-ffi`** semver: tracks Perry's minor today (`perry-ffi = "0.5"`
  for Perry `0.5.x`). Backwards-incompatible changes to anything
  documented in [`abi.md`](abi.md) bump perry-ffi *major* —
  independent of `perry-runtime`. Wrappers declare ABI `0.5` and use a tested
  Perry `0.5.x` revision of the crate; see [Consumption today](#consumption-today-v05x).
- **Manifest spec v1**: locked at `abiVersion: "0.5"`; missing field
  is warning-only in v0.5.x, hard error from v0.6.0. Schema changes
  bump the spec version (`v2`) and ship alongside a new manifest
  schema file.
- **Wrappers**: each ships independent semver; users `bun update` a
  binding without touching Perry.

## Consumption today (v0.5.x)

`perry-ffi` owns its public ABI types, but it is not currently available from
crates.io. Publishing it requires publishing the optional `perry-runtime`
dependency and its workspace dependency chain first. External wrappers use
the repository dependency that `perry native init` emits:

```toml
[dependencies]
perry-ffi = { git = "https://github.com/PerryTS/perry", branch = "main" }
```

Pin a tested Perry tag or commit for a released wrapper rather than following
`main` indefinitely. Leave the optional `runtime-link` feature off in a
production wrapper. It is primarily for wrapper test binaries that need Cargo
to pull in the runtime symbol provider; Perry supplies the runtime archive at
an application's final link step. See
[`abi.md`](abi.md#versioning-and-dependency-setup) for the exact contract.

## Limits

- Bindings are **build-time linked**. Perry doesn't `dlopen` plugins
  at runtime — the staticlib joins the link line, the binary stands
  on its own.
- Bindings can't bring their own JS runtime — they extend Perry's,
  not replace it. A binding that wants its own GC / event loop /
  threading is out of scope.
- Cross-target prebuilds are the binding author's responsibility.
  The scaffolded GitHub Actions workflow handles the common matrix
  (x86_64+aarch64 macOS/Linux + Windows); other targets need
  manual additions.

## Next pages

- [`abi.md`](abi.md) — the perry-ffi surface, reference grade.
- [`manifest-v1.md`](manifest-v1.md) — the `perry.nativeLibrary`
  schema, every field documented.
- [API reference](../api/reference.md) — auto-generated list of every
  stdlib symbol Perry implements.
