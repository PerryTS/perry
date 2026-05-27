# Test262 subset comparison (#799)

Runs the subset of ECMAScript [Test262](https://github.com/tc39/test262)
that's applicable to Perry's TS subset, in both Perry and Node, and
buckets the diff by category.

Companion to the Node-core radar (`../node-core/`, #800): that one pulls
Node's own `test/parallel` corpus to probe the `node:*` **APIs**; this one
pulls TC39's Test262 to probe the **language**. Both are coverage radars,
not gates — they point at the biggest gaps, they don't block merges.

## What runs

`features-applicable.txt` enumerates the Test262 feature tags whose tests
Perry can reasonably be expected to handle. The runner walks
`test/language` and `test/built-ins` (override with `--dir`) and includes
a case unless:

- it declares a `features:` tag that isn't on the applicable list (e.g.
  `Temporal`, `regexp-lookbehind`); or
- it carries a flag we can't honour as a plain script — `module` (needs
  ESM loader semantics), `CanBlockIsFalse`/`CanBlockIsTrue` (needs a
  multi-realm host); or
- it leans on a `$262` host intrinsic (`detachArrayBuffer`,
  `createRealm`, `evalScript`, the agent API) that neither bare runtime
  provides — those would throw under *both*, a false agreement; or
- it lives under an out-of-scope subtree (`intl402`, `staging`, `eval`,
  `Atomics`, `SharedArrayBuffer`, `Temporal`, `RegExp/lookbehind`).

Pass `--all-features` to ignore the feature allow-list and run every
discovered case (useful for measuring the raw denominator).

## How it works

Test262 cases are silent on success and `throw` on failure, so the
primary signal is **exit-code parity**, with stdout as a secondary
tiebreak for clean runs.

Each case needs a harness host that defines `Test262Error`, `assert`,
etc. The runner assembles each case the way TC39's own runner does —
concatenating the default harness (`sta.js` + `assert.js`), a tiny host
`preamble.js` (`print` / `$DONOTEVALUATE`), any `includes:` files, and
the test source into one script — then runs that **single script** under
both runtimes. `onlyStrict` cases get a `"use strict";` prologue;
`async` cases pull in `doneprintHandle.js`; `raw` cases run verbatim with
no harness. Because both runtimes load the *same* assembled script, the
differential compares the two runtimes' **builtins**, never their
harnesses.

Raw CommonJS/JS runs under Perry because Perry feeds user `.js` through
the native AOT pipeline (the same path `compilePackages` uses; see #668).

### Buckets

Test262 is full of **negative tests** where the *correct* behaviour is to
reject (a SyntaxError at parse, or a thrown error at runtime). So — unlike
the Node-core radar, which drops every case Node fails (`node-skip`) — this
runner buckets by Perry-vs-Node **agreement**:

- `pass`         — Perry agrees with Node: both ran clean (exit 0) with
                   matching stdout (positive case), **or** both rejected
                   (negative case — Node exits non-zero and Perry rejects
                   at compile *or* runtime).
- `diff`         — both ran clean (exit 0) but stdout differs.
- `runtime-fail` — verdict mismatch on a case Perry *compiled*: Node ran
                   clean but Perry threw, or Node rejected but Perry ran
                   clean (a missed negative).
- `compile-fail` — Perry refused to compile a case Node ran clean. (When
                   Node *also* rejected, Perry's compile rejection is the
                   correct answer and lands in `pass`.)
- `skip`         — couldn't assemble (missing include) or needs an
                   unsupported flag / `$262` host API. Excluded from the
                   parity verdict — never charged against Perry.

`parity_pct = pass / (pass + diff + runtime-fail + compile-fail)`. The
report also records `negative_agreements` (how many of the passes are
both-runtimes-correctly-rejected) so the language-correctness signal and
the negative-rejection signal stay legible.

## Files

- `features-applicable.txt` — curated allow-list of feature tags.
- `pinned-sha.txt` — the Test262 SHA the corpus is pulled from.
- `preamble.js` — host shims (`print`, `$DONOTEVALUATE`) prepended to
  every non-`raw` assembled case under both runtimes.
- `report.json` — written by the runner (a generated artifact; not
  committed).

## How to run locally

```bash
# 1. Vendor Test262 at the pinned SHA (large; not committed).
git clone --depth 1 https://github.com/tc39/test262 vendor/test262
(cd vendor/test262 && git fetch --depth 1 origin "$(cat ../../test-compat/test262/pinned-sha.txt)" \
   && git checkout FETCH_HEAD)   # optional: pin exactly

# 2. Build Perry.
cargo build --release -p perry -p perry-runtime -p perry-stdlib

# 3. Run the subset.
scripts/test262_subset.py --root vendor/test262                       # full default scope
scripts/test262_subset.py --root vendor/test262 --dir language/expressions
scripts/test262_subset.py --root vendor/test262 --max 500             # cap for a quick read
```

## What a CI job would do

1. Shallow-clones `tc39/test262` at `pinned-sha.txt`.
2. Builds Perry, runs `scripts/test262_subset.py`.
3. Uploads `report.json` as an artifact.
4. **Advisory** (non-required) — signal, not gating. Threshold-based
   gating can be added once the baseline is stable across a few runs.

Part of #793. Companion job to #800 (Node's own test corpus).
