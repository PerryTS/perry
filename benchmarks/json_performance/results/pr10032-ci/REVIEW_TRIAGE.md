# PR #10032 review triage

Review snapshot: head `b2111219c1a4353b05f35fc55d9305c55d2cb135`,
2026-09-09. Findings were checked against the source and saved runs.

## Confirmed and being corrected

- Sparse tape reads dropped lone surrogate escapes while the new batched parser
  preserved them. A 128-record compiled worker probe makes the first value empty
  while subsequent batched values keep `\ud800`; Node preserves every value.
  The fix shares the canonical escape decoder and the escaped-string constructor,
  including the guard against putting lone surrogates into the inline string form.
  Coverage exercises isolated, ascending, descending and forced materialization,
  escaped keys and duplicate keys, valid pairs and unmatched high/low surrogates.
- The new cross-materializer test also exposed a generic stringify bug for a
  lone-surrogate property name on an object containing nested values: it emitted
  a synthetic `field1` name. The UTF-8 key path now falls back to the existing
  byte escaper after validating WTF-8, with no managed allocation or callbacks.
  Invalid internal byte sequences are rejected before the output is changed.
- The moving-GC test's generic object hook could also fire on the fallback path.
  Train #10033 landed a separate batched-record hook that proves the intended
  producer ran. The correction now builds on that merged test unchanged.
- The active `run_dispatch_focus.py` now rejects repeat counts below one before
  creating output. CLI probes for zero and negative counts verify this behavior.
  Historical runner snapshots retain their original measured bytes.

## Historical artifacts

`merged-main-c82e613d2-rotating-r5` is an earlier development-host diagnostic run,
not quiet-host performance acceptance. Its old metadata contract is not rewritten
as though it were measured by the newer harness. Current accepted release runs
have the explicit engine and verification-count inventories and pass the current
summarizers. The existing disposition files identify superseded and inadmissible
runs; use the latest release report for performance claims.

Archived `with_lock.py` snapshots can fail closed with a traceback if an existing
lock has no owner file. They do not run a benchmark or replace that lock. Editing
an archived runner would invalidate its provenance; this limitation does not
change a completed window's quiet admission record.

Hostnames, installation paths and OS identifiers in these benchmark artifacts
identify the execution environment and commands. These fields are execution-environment metadata. They must not be confused
with an instruction to connect to any host.
The full process listings are diagnostic artifacts, not inputs to performance
calculations; future evidence should prefer the filtered workload lists already
stored in the admission window.

The contributor PR template's no-version-bump checkbox does not apply to this
maintainer change: CLAUDE.md explicitly requires a version and PR-number changeset
for every change landing on main. Docstring-percentage suggestions are not a
repository-required check; document the non-obvious correctness contracts.

## Validation status

The first correction run passed 280 tests and failed two new cases, both on
that synthetic-key stringify defect; its log is preserved separately. The expanded correction passes all 283 JSON tests, the matched release build,
and 14 compiled Node comparisons. Twelve compiled cases fail on the old binary.
Moving/protected-GC, full-GC, retained-output and cadence witnesses also pass.
See [the validation evidence](../pr10032-decoder-validation/README.md). Performance comparisons on b211 remain historical evidence;
the corrected build must be checked before replacing their conclusions.

PR #10032 was incorporated into main by audited train #10033 at `e7223f700`
and closed on 2026-09-09. The escaped-value/key correction therefore belongs in
a new PR. R1's complete quiet-host comparisons pass output validation but its
small-array sparse-read slowdown repeats (+2.65% in longer trials), so it is not
performance acceptance. R2 restores the tape scanner call boundary and must be
validated and measured against a fresh build of that merge.
