# Node core test subset comparison (#800)

For each `node:*` API that `perry-stdlib` claims to support, pull the
corresponding `test/parallel/test-<api>-*.js` files from the Node.js
repo, run them under **both** Perry and Node, and bucket the divergence.

This is a **coverage radar**, not a gate. Where the hand-authored
`test-parity/node-suite` cases probe whatever a human thought to write,
this runner exercises Node's *own* tests — the canonical definition of
correct behaviour, and the same corpus Deno and Bun lean on for their
Node-compat suites. Its job is to point at the biggest gaps, not to
block merges.

## How it works

Node's `test/parallel` cases are silent on success and `throw`
(exit != 0) on failure, so the primary signal is **exit-code parity**,
with stdout as a secondary tiebreak.

Each case does `require('../common')` — Node's ~1000-line test harness
that pulls in `net`, `worker_threads`, `process.binding`, etc. and does
not compile. The runner stages a small Perry-compilable CommonJS
**shim** (`shim/`) as `common/` next to each test. Both runtimes load
the shim (Node via its CJS loader; Perry compiles it natively), so the
differential compares the two runtimes' *builtins*, never their private
harnesses. The shim covers the most-used helpers (`mustCall`,
`platformTimeout`, platform flags, `skip`); anything missing makes the
test fail under Node too, landing it in `node-skip` (excluded — never
charged against Perry).

Running Node's raw CommonJS `.js` under Perry is possible because Perry
now feeds user `.js`/`.cjs` through the native AOT pipeline and rewrites
`require(...)` / `module.exports` to ESM (the same path used for
`compilePackages` CommonJS). See `collect_modules.rs` (the
`should_use_js_runtime` / `was_cjs_wrapped` gates) and #668.

### Buckets

- `pass`         — Node exits 0, Perry exits 0, stdout matches.
- `diff`         — both exit 0 but stdout differs.
- `runtime-fail` — Perry compiled but exited non-zero while Node passed.
- `compile-fail` — Perry refused to compile (parser / lower / codegen).
- `node-skip`    — Node itself failed under the shim (missing helper,
                   needs a flag/env, or genuinely env-dependent).
                   Excluded from the Perry verdict.

## Files

- `supported-apis.txt` — `perry-stdlib` API names included in the sweep.
- `pinned-version.txt` — Node tag/SHA the corpus is pulled from.
- `shim/` — Perry-compilable replacements for Node's `test/common/`
  (`index.js`, `tmpdir.js`, `fixtures.js`).
- `report.json` — written by the runner (a generated artifact; not
  committed).

## How to run locally

```bash
# 1. Vendor a subset of the Node tree (large; not committed). Sparse +
#    shallow keeps it to test/parallel + test/common + test/fixtures.
git clone --no-checkout --depth 1 --branch v22.x \
  --filter=blob:none https://github.com/nodejs/node vendor/nodejs
(cd vendor/nodejs && \
  git sparse-checkout set test/parallel test/common test/fixtures && \
  git checkout)

# 2. Build Perry.
cargo build --release -p perry -p perry-runtime -p perry-stdlib

# 3. Run the radar.
scripts/node_core_subset.py --root vendor/nodejs              # all supported APIs
scripts/node_core_subset.py --root vendor/nodejs --api path url
scripts/node_core_subset.py --root vendor/nodejs --max-per-api 25
```

## What a CI job would do

1. Sparse-checks-out `nodejs/node` at `pinned-version.txt`.
2. Builds Perry, runs `scripts/node_core_subset.py`.
3. Uploads `report.json` as an artifact.
4. **Advisory** (non-required) — signal, not gating. Threshold-based
   gating can be added once the baseline is stable across a few runs.

Part of #793. Companion to #799 (Test262).
