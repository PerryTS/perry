### Experimental patches for context-mode native compilation

**Experimental, opt-in.** This is a set of small forward-compat patches
to `perry-hir` that let the auto-optimizer's dyn-eval interpreter
ride along for `KnownLibraryCodegen` (ajv, fast-json-stringify,
find-my-way) call sites, and that track `const F = Function`-style
aliases of the global `Function` constructor through the
`globalThis.Function` path. Together they unblock compiling
`context-mode` (a Node.js MCP server) into a native 16 MB binary
with `node:sqlite` + FTS5 + HTTP MCP transport working end-to-end.

Full write-up — including the 7 upstream Perry issues worked around
and the file-level diff inventory — is in `CONTEXT-MODE-COMPAT.md` at
the repo root.

#### Why these patches exist

A real-world downstream consumer (`context-mode`, the MCP server
that lives next to this checkout) compiles fine into Node.js but
runs into three concrete gaps in the current `perry-hir`:

1. The auto-optimizer refuses to link the `dyn-eval` interpreter
   when only `KnownLibraryCodegen` (bucket-2) sites are present.
   It only consults `has_deferred_dynamic_code_sites()`. ajv's
   `new Function()` then throws at the first call.
2. Zod v4 (and any user code) does `const F = Function` to keep
   the global `Function` constructor reachable. `perry-hir` did
   not recognise that as a `Function` alias, so `new F(...)`
   degraded to a user-class `new` and lost the dyn-eval route.
3. `globalThis.Function = <local>` style aliases (also a real
   pattern, e.g. `globalThis.Function = Function`) were not
   tracked into the prototype-alias map, so the
   `is_global_this_value` branch in `alias_tracking.rs` would
   mis-classify the access.

Each is a small, isolated change. None alters the bucket-3
(`Deferred`) path that already works.

#### What changes

- **`crates/perry-hir/src/eval_classifier.rs`** — add
  `KNOWN_CODEGEN_SITE_COUNT` (an `AtomicUsize` sibling of the
  deferred-style sink) and a `has_known_codegen_sites()` accessor.
  `check_site` increments the counter on every
  `EvalBucket::KnownLibraryCodegen` site, mirroring the
  `record_deferred_aot_site` pattern.
- **`crates/perry-hir/src/lower/expr_new.rs`** — before the
  shadowed-by-user-binding check, ask
  `ctx.resolve_class_alias(&class_name)` whether the callee
  resolves to `"Function"`. If so, route through the
  `Function`-intrinsic path. The check is gated on a new local
  `function_alias` so it is short-circuited together with
  `force_global_intrinsic`.
- **`crates/perry-hir/src/destructuring/var_decl/alias_tracking.rs`**
  — extend the `is_global_this_value` `property.as_str()` arm
  to also accept `"Function"`, so a `globalThis.Function` lookup
  participates in the prototype-alias map.
- **`crates/perry/src/commands/compile/optimized_libs/freshness.rs`**
  — teach the freshness stamp about the new
  `has_known_codegen_sites()` (keyed as `knowngen=…`) and broaden
  the `cross_features.push("perry-runtime/dyn-eval"…)` gate from
  `has_deferred_dynamic_code_sites()` to
  `has_dyn_eval || has_known`. When the known-codegen bucket is
  the only one present, the regex engine still rides along (ajv
  uses regex literals too).
- **`crates/perry-hir/src/lib.rs`** — re-export
  `has_known_codegen_sites` next to
  `has_deferred_dynamic_code_sites`, so the new
  `freshness.rs` call site resolves through the same `pub use
  eval_classifier::{…}` block.

#### Status

This is an **experimental** patch series. The downstream build
(`context-mode` → `perry compile src/server-http.ts -o
context-mode`) compiles clean in ~2 minutes (release) / ~15s
(debug) and produces a 16.0 MB binary that responds to MCP
`initialize` / `tools/list` / `tools/call` with all 11 tools
working. No new Perry patches were required to extend from 2
tools to 11 — the five above are sufficient.

The series is deliberately scoped so it can be reverted without
leaving dead code: every change is additive, every new symbol
is re-exported from the same `pub use` block, and the
`cross_features` gate in `freshness.rs` is a pure superset of
the prior `has_deferred_dynamic_code_sites()` check.

#### Known limitation, not addressed

A separate `RangeError: toString() radix argument must be
between 2 and 36` GC crash on the *full* `context-mode/server.ts`
(4948 lines) is **not** fixed by this PR. The crash is real and
reproducible, but its root cause sits in `perry-codegen`'s
conservative-scan/object-shape path and a fix there is a
non-trivial GC change of its own. The downstream user works
around it by splitting the build into 4 sub-400-line tool
bundles that don't share an import graph with the crashing
modules — see `CONTEXT-MODE-COMPAT.md` for the file inventory
and the tool-level verification transcript.
