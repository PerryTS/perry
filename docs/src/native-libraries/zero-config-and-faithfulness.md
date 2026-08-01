# Zero-config bindings & the faithfulness marker

Status: **design note + landed increments.** This note records (a) the
current resolution behavior for bare npm imports that have a bundled
`perry-ext-*` binding, (b) the faithfulness (`compat`) marker landed
alongside it, and (c) the two policy decisions that are deliberately left
to a maintainer rather than flipped unilaterally.

## Background: the friction we set out to remove

A downstream command-line application compiled its CLIs with two
hand-maintained workarounds:

1. it kept a `perry.compilePackages` list of every AOT-compiled dependency, and
2. its build script physically moved
   `node_modules/{undici,node-forge,iovalkey,dotenv}` aside for the duration
   of each `perry compile`, because a bare `node_modules/<name>` copy used to
   shadow perry's bundled binding and trip the V8-free "JavaScript runtime"
   gate.

## Current behavior (map)

Resolution precedence for a bare `import 'X'` is documented at the top of
`crates/perry/well_known_bindings.toml` and implemented across:

- **The native short-circuit** — `perry_hir::is_native_module` (
  `crates/perry-hir/src/ir/constants.rs:439`) consults
  `perry_api_manifest::NATIVE_MODULES` (`crates/perry-api-manifest/src/entries.rs:30`).
  When `X` is in that list and **not** in `perry.compilePackages`, the import is
  marked native (`crates/perry/src/commands/compile/collect_modules.rs`, the
  `if import.is_native { … }` branch) and routed to the bundled binding —
  perry never walks `node_modules/X`.
- **File resolution** — `resolve::resolve_import`
  (`crates/perry/src/commands/compile/resolve.rs:1288`) returns `None` for a
  native module (line 1303), so a `node_modules/X` copy is only consulted when
  `X` is **not** native (or has been opted into `compilePackages`).
- **The V8-free gate** — `enforce_js_runtime_gate`
  (`crates/perry/src/commands/compile/bootstrap.rs:266`) hard-errors when the
  build reaches a `node_modules` JS module that is neither native nor in
  `compilePackages`. This is the intentional supply-chain boundary:
  AOT-compiling arbitrary dependency JS requires an explicit trust opt-in.

**Key finding:** `undici` (#7032), `node-forge` (#7033), `iovalkey`, and
`dotenv` were all added to `NATIVE_MODULES` in the last few days, so on current
`main` **all four already resolve to their bundled bindings even when a
`node_modules/<name>` copy is present** — verified empirically (`Found N
module(s): N native, 0 JavaScript`, no error, no `node_modules` move). The
"binding gets shadowed" problem that offload worked around is already gone.
That offload dance, and omitting these packages from `compilePackages`, are
now **stale** (see the cleanup section below).

That reframes the original ask. The literal "convert today's hard error into a
working binding build" is already the behavior for anything in `NATIVE_MODULES`
(which is kept in lock-step with the well-known table). What was missing is the
**safety and transparency** around it: perry auto-prefers *every* binding —
including the documented **subset** wrappers (`undici`, `node-forge`,
`lru-cache`) — silently, with no signal that the wrapper is not a full drop-in.

## Landed: the `compat` faithfulness marker

`well_known_bindings.toml` entries may now declare:

```toml
[bindings.dotenv]
crate = "perry-ext-dotenv"
lib   = "perry_ext_dotenv"
compat = "full"      # audited complete drop-in — safe to auto-prefer silently
```

- `compat = "full"` — audited complete drop-in for the package's public API.
- `compat = "partial"` — ports a subset, or not yet audited.
- **absent ⇒ `partial`** (conservative; a binding never becomes faithful by
  accident).

Parsed into `BindingCompat` on `WellKnownBinding`
(`crates/perry/src/commands/compile/well_known.rs`) with an `is_faithful()`
helper. Audited this pass: `dotenv`, `nanoid`, `slugify`, `uuid` → `full`;
`undici`, `node-forge`, `lru-cache` → explicit `partial` with rationale
comments; everything else left at the `partial` default.

Two consumers make the marker load-bearing, **both additive** (default build
byte-identical):

1. **Transparency note.** When a bare import is served by a `partial` binding
   *and* a `node_modules/<pkg>` copy is on disk, perry prints one note per
   package after collection
   (`crates/perry/src/commands/compile/run_pipeline.rs`), pointing at the
   `compilePackages` escape hatch. `full` bindings and imports with no local
   copy stay silent.
2. **Strict opt-in.** `PERRY_REQUIRE_FAITHFUL_BINDINGS=1` turns that note into a
   hard error — perry refuses to auto-prefer a `partial` binding over an
   installed copy. This is the provable "an unfaithful binding does *not*
   silently win" behavior; it is **off by default** so nothing currently
   relying on a partial binding breaks.

## Landed: auto-compile is the default

A project that does not pin a `perry.compilePackages` value now gets its whole
reachable `node_modules` graph compiled automatically — the host_config loader
injects the universal `"*"` (and auto-satisfies the `allow.compilePackages`
two-key check) when no explicit value is present. The existing #3527 wildcard
expansion then materializes `"*"` into concrete installed package names,
skipping natively-shimmed packages so bindings keep winning.

`perry.compilePackages: "auto"` (also `"all"` / `true`) and
`perry.allow.compilePackages: true` are the explicit spelling of that default —
useful to document intent or to re-enable auto-compile inside a config that
would otherwise constrain it.

Opt out to the old "listed packages only" posture with an explicit
`compilePackages` array, or `compilePackages: false` / `[]` to compile nothing
and re-arm the V8-free gate.

## Policy decisions left to the maintainer

These are **not** flipped here — they change the security/behavior contract and
want a human sign-off.

### 1. Should auto-preference of *partial* bindings require opt-in?

Today perry auto-prefers a partial binding (`undici`, `node-forge`, …) over a
user's real `node_modules` copy silently. The conservative alternative is to
make `PERRY_REQUIRE_FAITHFUL_BINDINGS` behavior the **default** — a partial
binding would error unless the user opts into either the binding or
`compilePackages`. That is *safer* (no silent subset substitution) but **would
break downstream consumers today**, because a dependent application can rely on
the `undici` and `node-forge` bindings being auto-preferred, and those wrappers
are, by design, subsets. So the two cannot both be true at once:

> "the four natively-shimmed packages just work with zero config" **and** "a
> partial binding refuses to auto-prefer"

are mutually exclusive while `undici`/`node-forge` remain `partial`. The honest
paths forward are: (a) keep auto-prefer-with-note as the default (current
choice) and let strict mode be opt-in; (b) complete the `undici`/`node-forge`
wrappers to `full` and *then* make strict the default; or (c) make strict the
default but ship a curated allowlist of "trusted partial" bindings. Recommend
(a) now, (b) as the target.

### 2. Should auto-compile be the default? — RESOLVED: yes.

Resolved by the owner: **auto-compile is the default.** Perry is a compiler,
not a supply-chain gate — faithfully compiling code that is already in
`node_modules` is not perry's call to block, and "this package is not on a
list" is not an error. Supply-chain hygiene (pinning, lockfiles, review,
dedicated tooling) is the user's responsibility, not a hand-maintained perry
allowlist. The V8-free gate is retained only as an opt-in constraint
(`compilePackages: []` / an explicit array) and for genuinely unsupported
situations, not for "unlisted".

Possible future layer (not built): perry could integrate a package-advisory
data source to **warn** about known-malicious packages during compile — a
telemetry/warning layer, never a build-blocking gate.

## Workarounds this removes for dependent projects

Once a project builds against a perry with these changes, its perry config
collapses to **nothing**:

- delete any `node_modules` offload/restore dance from the project's build
  script — the natively-shimmed packages resolve to bindings on their own (they
  are in `NATIVE_MODULES`), so no relocation is needed; call `perry compile`
  directly;
- delete the entire `perry` block from `package.json` — both
  `perry.compilePackages` and the mirrored `perry.allow.compilePackages`. Under
  auto-compile the reachable deps compile with no list;
- expect the informational per-package note for `undici` / `node-forge` /
  `iovalkey` (all `partial` bindings). It is not an error. Note that
  `lru-cache` is marked `partial` on this base (its faithful port, #7136, is
  not yet in `main`); once #7136 lands its covered option surface can move to
  `full`. A project that would rather compile the real `lru-cache` JS than use
  the binding can list just `lru-cache` in `compilePackages`.
