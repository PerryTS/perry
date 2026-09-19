**GC tooling: `unrooted_local_shape.py` had two independent ways of not firing.**
Both are the fourth shape in CLAUDE.md's "★ Four ways a gate can be unable to fail" —
the job is genuinely green.

**#10713 — `--no-raise-vs <base>` never looked at the code.** It read the merge
base's recorded baseline and the checked-out one and compared *those two numbers*.
A branch that added findings without touching the baseline therefore compared 561
against 561 and printed `no ceiling raised`, while `--check`, seconds later in the
same `run_lint_gates.sh` invocation on the same worktree, failed with
`REGRESSION: 563 findings exceeds baseline 561`. Reproduced by appending five
planted shapes to `perry-ext-http/src/response_headers.rs` and leaving the baseline
alone: `--check` exits 1, `--no-raise-vs origin/main` exits 0. The variant whose
whole purpose is catching a rise against the base could not see one, because the
only number it ever read was one the diff had no reason to move. It now scans the
worktree and compares the measured total and per-file counts against the base's
recorded ceilings — the same yardstick `--check` uses, so the two forms agree — and
prints the resolved base SHA with both the recorded and the measured totals, so a
reader can tell a real comparison from a vacuous one. The comparison is skipped only
across the audited schema-1 migration, where the two sides were measured by
different detectors.

**#10713, second hole — `--no-raise-vs ""` compared nothing and exited 0.** The
dispatch was `if args.no_raise_vs:`, testing *truthiness*, and the empty string an
unset `$BASE_SHA` expands to is falsy. The mode was never entered: no ref resolved,
no baseline read. The script fell through to the plain report, printed an ordinary
finding table and passed. Now `is not None`, and `resolve_ref` rejects an empty ref
in the same words it rejects an unfetched one — `raw_handle_debt.py`'s `git_show`
made the argument first: *a comparison that did not happen, reported as a pass, must
be a RED build instead*. An unresolvable ref was already handled; an empty one was
not, because nothing called the guard.

**#10715 — the detector was line-oriented, so `rustfmt` could hide a finding.**
`LET_BIND` was matched per line. Once a binding sits a few levels deep, or carries a
type annotation, rustfmt breaks it after the `=` and the head line has no right-hand
side: nothing matched, the local was never tracked, and the finding vanished. That
is a false negative bought with an indent, and it made the deepest-nested code —
where rooting bugs live — the least scanned. It bit for real on #10668, where a
genuine rooting fix had to be hoisted into a top-level function (`build_set_cookie_array`)
purely to keep its binding on one line and stay visible. A `let` is now folded back
into one statement before matching. Statements containing a brace are still read line
by line, on purpose: a closure, `match` or struct-literal initializer carries its own
bindings and collection points, and folding those into a single expression would
trade this blind spot for a strictly larger one.

**The measured surface rises from 558 to 581 findings across 85 files** (was 80), and
the baseline is re-pinned at 581 under an audited **schema 2 → 3 migration**. This is
not a loosened ratchet, and the distinction matters: *the old 561 was produced by a
weaker detector*. Comparing 581 against it compares two different yardsticks, which is
exactly why the script already carries the audited-migration exemption — the same
situation as the 1 → 2 migration, for the same reason. The ratchet's job is unchanged:
it still fails on finding 582, verified by planting one
(`REGRESSION: 582 findings exceeds baseline 581`).

The exemption is now an explicit `AUDITED_MIGRATIONS` list rather than a single
hard-coded `(1, BASELINE_SCHEMA)` pair, so each migration is named with its reason and
every unlisted schema change is still rejected. `--self-test` asserts both that 2 → 4
is refused and that `BASELINE_SCHEMA` cannot be bumped without naming its own
migration — otherwise a renumber would exempt every PR from the ratchet.

The number moved in both directions:

- **+34 newly visible**, led by `perry-stdlib/src/events.rs` (6 → 13),
  `perry-ext-node-forge` (20 → 24) and five files that recorded nothing at all.
  **Two were inspected and are genuine unrooted-across-allocation shapes**;
  `perry-ext-fastify/src/context.rs:750` is one: `let obj: *mut ObjectHeader =`
  wrapped by its own type annotation, with `obj` then held across `alloc_string` in
  the loop below it. Nothing about that code was safe; only its line breaks hid it.
  The other 32 are **unaudited exposure surface, not known bugs** — the number has
  always been a surface, not a bug count, and these 32 have simply never been looked
  at because no instrument could see them.
- **−11 false positives** in `perry-stdlib/src/ioredis.rs` (14 → 3), the same defect
  inverted: a wrapped *shadowing* `let err_str =` matched nothing either, so the dead
  identity from the earlier binding of that name stayed live and every use of the
  fresh one was reported against it.

**Self-test.** The old `--self-test` passed on the day the live check was fooled,
which is the whole problem, so each fix plants the defect it fixes and fails without
it, verified by reverting each one in isolation:

- `planted_wrapped_binding` — `let object =` with the initializer on the next line,
  taken from the live `perry-ext-ws` `js_ws_server_address` site. Not flagged before
  the fold.
- `clean_wrapped_shadow_rebinds` — the ioredis shape. Flagged before the fold.
- `planted_inside_wrapped_closure` — a binding inside a multi-line closure body,
  which must stay visible; it goes dark if the fold is ever let past a brace.
- `_self_test_no_raise_vs` — drives the real `no_raise_vs` over the observed
  combination: both recorded baselines identical at 561, worktree measuring 563.
  Returns 0 without the measured comparison.
- `_self_test_empty_ref_dispatch` — `--no-raise-vs ""` through `main()`. Returns 0
  under the truthiness dispatch. Guarding inside `resolve_ref` alone does not cover
  this, because nothing called it, and `git rev-parse` rejects an empty ref anyway.

**Every previous green from the `--no-raise-vs` arm was vacuous**, including the one
that ran on #10668. The gate's history is not evidence about the code it ran over.

Closes #10713
Closes #10715
