**`--opt-report` now says `spec_no_call_sites` where it used to say nothing** (#7111).

A function whose call sites were all removed by an earlier pass — the inliner, then `unroll_static_loops` constant-folding what it produced — has no entry in `spec_facts.call_sites`, which is built by walking `hir.init` and every body for direct `Call` expressions. The spec-ABI decision loop `continue`d on that **before** constructing any `TypedCloneRejectionReason`, so nothing reached `record_typed_clone_rejection` and nothing reached `opt_report::deny_named`.

The result was silence, and silence is the one answer this report cannot afford: it is indistinguishable from "not analysed" and from "analysed and denied". A reader now sees that the loop was reached and had nothing to decide.

Measured on a four-line fixture whose only call to `tiny()` is inlined and folded away:

| | entries | rules |
|---|--:|---|
| before | 1 | `not_index_used_or_bounded` |
| after | 2 | `not_index_used_or_bounded`, **`spec_no_call_sites`** |

The new `TypedCloneRejectionReason::SpecNoCallSites` is deliberately not framed as a denial — its doc says so — because there is nothing to specialise *for*; it is reported so the absence is legible rather than inferred.

Worth recording for the next person: `--opt-report` writes to **stderr**, deliberately, so it cannot contaminate a `--format json` stdout payload or piped program output. Three of my attempts to observe it came back empty because they were running under `2>/dev/null`. That is also the likeliest reason a report looks like it "did not render".

`cargo test -p perry-codegen --lib` 851 passed / 0 failed; fmt and file-size clean.
