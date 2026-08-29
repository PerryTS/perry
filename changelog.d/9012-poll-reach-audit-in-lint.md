The GC poll-reach audit is unblocked and now runs on every PR.

`--audit-poll-reach` was exiting 2 on `main`: #8965 added
`js_string_concat_value_box` without listing it in `POLL_CAPABLE_RUNTIME`. It is
the exact analogue of `js_string_concat_box` listed directly above it — an SSO
fast arm, then `js_string_concat_value` as the fallback (`string/concat.rs:496`),
and that callee was already listed — so it was an omission at introduction.

That audit is an early step of `gc-root-dominance.yml`, ahead of the compiler
build, so its failure masks the corpus and checker the workflow exists for
(#8821). The issue dates the red to 2026-08-15; that was the third wave, cleared
by #8823 on 08-25, and this is a fresh recurrence from 08-28 — the second
occurrence of the pattern the list exists to catch.

Because recurrence is the norm, the audit now also runs in `lint`. It is static,
build-free and 0.6 s including its self-test, whereas `gc-root-dominance.yml` is
label-gated behind `run-extended-tests` and therefore skipped on every PR, able
to report only after the fact on a scheduled `main` run. This is the placement
argument the runtime GC-pointer holder audit already makes in that file: cheap
and build-free, so it belongs in a required context. The expensive corpus arms
are unchanged, as are #8821's remaining questions and #8809/#8810's findings.
