# PR #10032 CI disposition

The initial CI run on `58b602a78` found four new raw-handle accesses in the JSON
tests. They were changed to `RuntimeHandle::with_const_ptr` / `with_mut_ptr`
around the self-rooting parse and lazy-materialization entry points. The local
raw-handle gate returns to 949 sites, matching the unchanged baseline and all
110 per-module ceilings. No ratchet ceiling or exception was raised.
The corrected tests passed all 279 JSON runtime tests again; see the
[single-threaded release test log](scoped-handle-json-tests.log). The production
runtime files and measured executables are unchanged by this test-only edit.

Two other failures reproduce on the current main commit `f2dc03582`:

- Public benchmark evidence freshness: "public artifact benchmark inputs
  changed" in both the [main lint job](https://github.com/PerryTS/perry/actions/runs/34370329602/job/102529707074)
  and [PR lint job](https://github.com/PerryTS/perry/actions/runs/34374267049/job/102543279648).
  The source and harness input fingerprints match main exactly; see
  [the recorded comparison](inherited-public-baseline-inputs.json).
- `native_stack::tests::stack_top_respects_custom_thread_stack_sizes` fails
  "bound must belong to this worker" at `native_stack.rs:53` in both the
  [main runtime test job](https://github.com/PerryTS/perry/actions/runs/34370329602/job/102529707040)
  and [PR runtime test job](https://github.com/PerryTS/perry/actions/runs/34374267049/job/102543279905).
  Main passes 3,486 other runtime tests; the PR passes 3,494, including the new
  JSON tests. Both report four ignored tests and this one failure.

These findings explain the inherited failures; they do not waive required
checks or claim that the PR gate is green. No published artifact fingerprint,
benchmark measurement, or failing runtime assertion has been rewritten to hide
them. The public-baseline refresh is also recorded as outstanding in PR #9998.

The later run on b211 repeats the two inherited failures: [lint](https://github.com/PerryTS/perry/actions/runs/34376199953/job/102561570002)
reports stale public evidence but confirms raw-handle debt 949/949;
[cargo-test](https://github.com/PerryTS/perry/actions/runs/34376199953/job/102561569981)
again passes 3,494 tests with only the same native-stack assertion failing.
The subsequent [review corrections](REVIEW_TRIAGE.md) have their own local
validation; they do not remove either inherited gate failure.
