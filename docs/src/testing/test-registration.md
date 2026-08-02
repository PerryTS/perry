# Test Registration (dark tests)

> **A new test file must be registered in its suite's registry, or it will not
> run.** Most of Perry's suites glob their inputs, but four do not — they read an
> explicit list. A file added to one of those without its registry line is not a
> failing test, it is *no test at all*.

`scripts/check_test_registration.py` enforces this. It runs in the `lint` job on
every pull request, takes about a fifth of a second, and needs no compiler, no
Node and no build.

```bash
python3 scripts/check_test_registration.py             # the gate
python3 scripts/check_test_registration.py --list      # what is in scope, and what is not
python3 scripts/check_test_registration.py --self-test # prove the gate can still fail
```

## Why this exists

A dark test is invisible in exactly the way that matters. The PR is green. The
reviewer sees a witness in the diff and a passing CI run next to it and reads
the two together as "covered". Nothing says otherwise, because nothing ran.

It happened four times against `test-parity/gc_repsel_corpus.txt` alone:

| PR | What went dark |
|----|----------------|
| #7192 | a `test_gap_gc_*` stale-root witness, dark from merge |
| #7216 | a second one, same shape |
| #7252 | `test_gap_gc_call_argument_rooting`, caught only once #7192/#7216's own registration assert reached `main` |
| #7270 / #7271 | two more (rest-argument and same-module call-argument rooting), caught by the maintainer at merge |

Two partial gates already existed and neither could catch the pull request that
needed it. `scripts/gc_repsel_matrix.sh` auto-detects unregistered
`test_gap_repsel_*` / `test_gap_specabi_*` files, and `gc-moving-witnesses.yml`
adds `test_gap_gc_*` — but both sit behind a full release build of the compiler,
behind a changed-paths relevance filter, and in workflows that are not in branch
protection's required contexts. This script is the cheap half of those checks,
pulled out to somewhere it can block a merge, and generalised to the other three
places in the tree with the same shape.

## Registry-driven suites

Run `--list` for the authoritative version with every exclusion and its reason.

| Registry | Candidate files | Runner |
|----------|-----------------|--------|
| `test-parity/gc_repsel_corpus.txt` | `test-files/test_gap_{gc,repsel,specabi}_*.ts` | `scripts/gc_repsel_matrix.sh` (`gc-stress`, `gc-moving-witnesses`) |
| `test-features/feature_matrix.toml` | `test-features/probes/**/*.ts` | `scripts/gen_feature_matrix.py` (`feature-matrix`) |
| `benchmarks/compiler_output/workloads.toml` | `benchmarks/compiler_output/fixtures/**/*.ts` | `scripts/compiler_output_regression.py` (`compiler-output-regression`) |
| a `mod` declaration in the parent module | `crates/*/**/tests/**/*.rs` below a suite root | `cargo test` |

The last one is the Rust analogue and it is worth spelling out: cargo
auto-discovers `crates/<crate>/tests/<suite>.rs`, but a file one level deeper —
a suite's module directory, or a `#[cfg(test)]` submodule under `src/` — only
compiles if a `mod` declaration names it. Without one, rustc never parses the
file. It is not dead code; it is not code. No warning fires.

Everything else is glob-driven and cannot go dark. `--list` names those too, so
"considered and safe" is distinguishable from "never looked at".

## What the gate does, and what it refuses to do

Per CLAUDE.md's *four ways a gate can be unable to fail*:

- **It cannot pass vacuously.** Every mechanism declares a floor on its
  candidate set and fails if the glob stops matching. "0 dark files over 0
  candidates" and "0 dark files over 157 candidates" print the same verdict and
  mean opposite things, so the summary always names the counts:
  `checked 157 files against 4 registries`.
- **It is proven able to fail.** `--self-test` plants an unregistered file into
  each of the four mechanisms — over the real registries, via an in-memory
  overlay, so nothing touches your working tree — asserts the gate names it,
  then removes it and asserts the gate goes green again. It also asserts a
  collapsed candidate set fails, a stale exclusion fails, and a registry entry
  whose file is gone fails.
- **Exclusions are named, not counted.** A numeric threshold cannot tell a new
  dark file from an old one: fix one, add one, and the tally is unchanged. Every
  non-registered candidate is listed in the script with a reason. A stale
  exclusion — one that matches no file on disk — is itself a failure, so an
  excuse cannot outlive the file it excuses.
- **It runs where it blocks.** It is a step in `lint`, which is already a
  required context. That placement is deliberate: forgetting to add a new job to
  branch protection is hazard 2, and `gc-root-dominance` sat red and blocking
  nothing for days because of it. This gate adds no new job, so there is no
  branch-protection step left to forget.

## When it fires

You will see something like:

```
TEST REGISTRATION: a test file exists that nothing runs.

  - DARK TEST test-files/test_gap_gc_rest_argument_rooting.ts
      exists on disk but is not registered in test-parity/gc_repsel_corpus.txt, so
      scripts/gc_repsel_matrix.sh (gc-stress, gc-moving-witnesses) never runs it.
      Register it there, or add it to this script's `gc-repsel-corpus` exclusions
      with a reason.
```

Two ways out, and only two:

1. **Register it.** Add the line to the named registry. This is almost always
   the right answer — you wrote the file to run.
2. **Exclude it, with a reason.** If the file is genuinely a helper (a fixture
   imported by a registered test, a vendored dependency, a workload driven by a
   *different* registry), add it to that mechanism's `exclusions` dict in
   `scripts/check_test_registration.py` and say why in prose. Reviewers read
   that text; "excluded" on its own is not an answer.

There is deliberately no third way. No threshold to bump, no `--allow-dark`, no
environment variable.

## Not covered

`tests/*.sh`, `tests/*.py` and `tests/*.ts` have no registry to diff against —
143 of the 171 files there are referenced by nothing in the tree. That is a
separate archaeology problem (triage each one: wire it up, or delete it), not an
unregistered-file problem, and inventing a registry for it retroactively would
make this gate red on day one for reasons that have nothing to do with the four
dark witnesses it was written for.
