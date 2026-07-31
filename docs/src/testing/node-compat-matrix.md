# Node builtin-module Compatibility Matrix

Perry reimplements the `node:*` module surface natively. The **compatibility
matrix** (`scripts/node_compat_matrix.mjs`) measures — against a *pinned,
verified* Node — how faithfully Perry reproduces each builtin's **export
shape**, for **both** import forms (`M` and `node:M`).

Its value is **breadth**: every builtin, both forms, one pinned oracle. It is
the systematic version of tracker
[#812](https://github.com/skelpo/perry/issues/812) ("42-module behavioral
matrix"). Deep *behavioral* parity lives in the hand-authored node-suite
(`run_parity_tests.sh`); this harness is a wide, shallow **shape** sweep.

## To check one module fast

```bash
node scripts/node_compat_matrix.mjs --module fs
```

That is the command to reach for while iterating on a single builtin. The
pinned Node download happens once and is cached under `.cache/node-pin/`, so
subsequent runs are just Perry compile + run. Narrow further:

```bash
node scripts/node_compat_matrix.mjs --module fs,path,crypto            # a few modules
node scripts/node_compat_matrix.mjs --module fs --method readFileSync,promises  # only these exports
node scripts/node_compat_matrix.mjs --only fs.readFileSync,path.join   # combined mod.export form
```

A `--method`/`--only` subset narrows the fingerprint to those exports for a
sub-second "did my change fix `node:fs.readFileSync`?" loop. Because it changes
the fingerprint semantics, it is a **print-only diagnostic** — it is refused
for `--check`/`--update-baseline`.

## Full sweep and the CI gate

```bash
node scripts/node_compat_matrix.mjs                 # whole matrix + summary table
node scripts/node_compat_matrix.mjs --check         # exit 1 on regressions vs the baseline
node scripts/node_compat_matrix.mjs --update-baseline   # rewrite the committed baseline
```

The harness needs the release binary (`cargo build --release -p perry`). The
baseline lives at `test-parity/node-compat-matrix.baseline.json`; the CI job
`.github/workflows/node-compat-matrix.yml` runs `--check` nightly in its own
job (so the pinned-Node download never slows the main test job).

A baseline is **scoped to the platform and Node line it was generated
against** — platform-dependent surfaces (`os`, `path/win32`, `dgram`, `fs`,
`inspector`) and version-dependent export shapes only compare meaningfully
within that scope. `--check` therefore **refuses** to run when the baseline's
`platform`/`nodeVersion` header does not match the current run (a
cross-platform comparison would surface phantom regressions or mask real
ones), and refuses a vacuously green pass when a full sweep processed fewer
modules than the baseline records. The committed baseline is `darwin-arm64`, so
the nightly gate runs on a `macos-14` (Apple Silicon) runner to match it; move
the baseline to another platform by regenerating it there and pointing the job
at a matching runner.

`--check` fails when a baselined cell got **strictly worse** (per a severity
order: `match` → `shape-diff` → `perry-unresolved`) or when a **prefix-parity**
invariant that previously held broke. Improvements are always accepted and are
folded in by `--update-baseline`. A `--module` selector scopes
`--check`/`--update-baseline` to just that slice — a single-module refresh
never rewrites the whole baseline.

## The pin

The oracle is the **official nodejs.org dist tarball** for the host platform, a
*binary* pin (not a source checkout — we measure shape against the runtime Node
actually ships). It is recorded in `external-tools.json` under `tools.node`
with a `sha512` SRI per platform, matching that file's existing convention. The
runner downloads it, verifies the SRI, and caches it under `.cache/node-pin/`
(gitignored). nodejs.org also publishes `SHASUMS256.txt` (sha256), which is
cross-checked against every pinned asset at pin time.

Currently pinned: **Node 26.5.1** (the latest CURRENT stable). This is a
separate concern from `.node-version` (26.5.0), which pins the gap-suite /
node-suite oracle; the compat matrix carries its own "latest stable" pin.

## The fingerprint

For a module `M` and a form, the probe does:

```ts
import * as ns from "M"          // or "node:M"
// sorted list of `<exportName>:<typeof export[name]>` over Object.keys(ns),
// plus `default:<typeof ns.default>`, wrapped in a __FP__...__FP__ sentinel.
```

Two fingerprints are **equal** iff the two module namespaces have the same
export names with the same `typeof` for each, and the same default-export
`typeof`. It is a **shape** fingerprint (names + typeofs), not deep behavior.
The sentinel line means environmental warnings on stdout/stderr never touch the
compare — no output-normalization needed.

Each `(module, form)` cell gets a status:

| status | meaning |
| --- | --- |
| `match` | Perry fingerprint == oracle fingerprint |
| `shape-diff` | both resolved, fingerprints differ (a real shape gap) |
| `perry-unresolved` | Node resolved, Perry did not compile/run |
| `perry-extra` | Perry resolved a form the oracle did not |
| `both-unresolved` | neither resolved (neutral) |
| `skip` | curated skip — see `test-parity/node-compat-matrix.skip.json` |

Modules that cannot be meaningfully fingerprinted by a bare `import * as m`
(side-effectful on import, or shape depends on constructor args) go in the skip
JSON **with a reason** rather than silently passing.

## The prefixed / unprefixed invariant

Node treats `M` and `node:M` identically for builtins, and Perry's
`is_native_module` strips the `node:` prefix — so the two forms **must** agree.
The runner computes both and records `prefixParity`. A `false` there is a **real
Perry bug** and fails `--check` if it previously held.

## Bumping the pinned Node

1. Edit `tools.node.version` in `external-tools.json` and refresh the
   per-platform `sha512` SRI (download each dist tarball, verify its sha256
   against that version's `SHASUMS256.txt`, then record the recomputed sha512).
2. `node scripts/node_compat_matrix.mjs --update-baseline`.
3. **Review the diff.** A Node bump legitimately changes fingerprints (new
   exports, typeof changes); confirm the deltas are Node's, not Perry
   regressions, before committing.
