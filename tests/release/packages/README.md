# `tests/release/packages/`

Fixtures for tier 3 of `scripts/release_sweep.sh` — verify that real npm
packages compile + run under Perry and produce byte-identical output to
Node. This is the load-bearing tier for the 0.6.0 bump: parity
microbenchmarks already pass, but real-package fixtures keep catching
bugs the gap suite misses (see #585 / #588 / #589).

## Layout

```
tests/release/packages/
  README.md           ← you are here
  _harness.sh         ← fixture iterator, called by tier 3 of release_sweep.sh
  hono-basic/
    package.json      ← pin a known-good version of the package
    entry.ts          ← Perry-compilable TypeScript that exercises the package
    expected.txt      ← byte-exact stdout
    fixture.sh        ← per-fixture runner: install → compile → run → diff
  <next-fixture>/
    ...
```

## Adding a new fixture

1. `mkdir tests/release/packages/<name>` and add at minimum:
   - `package.json` declaring the package as a dependency, plus a
     `"perry": { "compilePackages": [...] }` entry pointing at the
     package(s) Perry should compile natively (instead of routing through
     QuickJS).
   - `entry.ts` — a TS file that imports the package and prints
     deterministic output. Avoid timing-dependent values, hash-of-Date,
     iteration order over Maps you didn't seed, etc. Anything that ought
     to be byte-identical between Perry and Node.
   - `expected.txt` — Node's stdout. Generated locally with
     `node entry.ts > expected.txt` after you've vetted the output.
   - `fixture.sh` — copy the existing pattern (see hono-basic). Must:
     - `cd "$(dirname "$0")"` so relative paths work.
     - Honor `PERRY_BIN` (default `target/release/perry` from repo root).
     - On success: print one `PASS <fixture-name>` line and exit 0.
     - On failure: print `FAIL <fixture-name>`, the diff, exit 1.

   Linker-only regressions may omit `expected.txt` and stop after
   compile/link plus targeted symbol inspection; document in the fixture
   why the binary is not executed.
2. Run `./tests/release/packages/_harness.sh --filter <name>` to verify
   the fixture works in isolation before checking it in.
3. Run `./scripts/release_sweep.sh --tier=3` to verify it runs through the
   tier wrapper.

## Backends

Per the 0.6.0 plan, fixtures use mock/embedded backends — no Docker. For
package categories that need a real backend, the convention is:

- **Filesystem-backed** (sqlite for drizzle, etc.): include the dep in
  `package.json`, no extra setup.
- **Network-backed** (redis, MinIO, mysql): the fixture's `fixture.sh`
  is responsible for launching/tearing down the backend. Skip cleanly
  with `exit 0` + `SKIP <name> reason` if the backend binary isn't
  available on `PATH` (the harness counts skips separately from fails).
- **Compile-backed** (drizzle pg-proxy with a Perry-built echo server):
  build the helper into the fixture directory and launch it from
  `fixture.sh` the same way.

## Running

```
./tests/release/packages/_harness.sh                 # all fixtures
./tests/release/packages/_harness.sh --filter hono   # one fixture
PERRY_TEST_SUMMARY_OUT=/tmp/x.json _harness.sh       # emit JSON summary
```

The harness defines its own pass/fail/skip totals; downstream consumers
(release_sweep tier 3) only care about the JSON summary.

The `effect-basic` and `ink-link-smoke` fixtures also have named CI jobs in
`.github/workflows/test.yml`. They run on release tags, on manual dispatch with
`run_extended_tests=true`, and on PRs with the `run-extended-tests` label. The
Effect job opts into a currently advisory compile/run signal with
`PERRY_EFFECT_BASIC_ADVISORY=1`; the default tier-3 sweep records it as SKIP so
known Effect end-to-end gaps do not block unrelated package releases. The Ink
job intentionally stops at compile/link plus symbol inspection; end-to-end Ink
rendering remains tracked separately from the release fixture contract.
