The changeset gate now rejects the `0000-` changelog placeholder and warns when
an added fragment carries neither this PR's number nor a plausible backfill's.

It previously accepted any `changelog.d/<digits>-<slug>.md` — *some* digits, not
this PR's number — so both the unfilled placeholder and another PR's number
passed. Nothing reads a fragment until a release is cut, at which point the
change is attributed to the wrong PR; #8978 recorded four in one day, every one
caught by eye rather than by a gate.

An all-zero prefix is a hard failure: no PR is ever number 0, so it is always
the placeholder. A mismatched real number is only a warning, because a backfill
(#8973 renamed three fragments to numbers that were not its own) and a stacked
PR naming its parent both legitimately carry one — a strict rule would block the
PR that repairs the problem.

The logic moved out of inline YAML into `scripts/check_changeset_fragment.sh`,
whose `--self-test` exercises the same function the gate calls rather than a
copy, and runs on every tier instead of only on `pull_request`. The enforcement
invocation takes `${{ }}` arguments, so `run_lint_gates.sh` skips it locally via
the mechanism added in #9009 while still running the self-test.
