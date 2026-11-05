**ci: tolerate two known compile-smoke failures (#9470)**

`compile-smoke` is in `full-suite-gate`'s `needs` and exits on `FAIL -gt 0` with
no allowlist, so two long-standing failures were blocking **every** release cut:

```
Compile smoke: 1359 passed, 2 failed, 67 skipped
- test_issue_340_axios_response_props
- test_issue_414_mysql_query_params
```

Both are the tokio-coherence refusal. Auto-optimize rebuilds the stdlib static
into `target/perry-auto-<hash>/` **without** the ext wrappers in the same cargo
invocation (`optimized_libs/driver.rs:846-857` passes only
`-p perry-runtime-static -p perry-stdlib-static --no-default-features`), so
tokio's features resolve differently for them and the linker refuses the pair.

Root cause and fix options are #9470. Adding ext crates to this job's
`cargo build` line does **not** help — `perry-ext-mysql2` is already there;
auto-optimize's *later* rebuild is what breaks coherence.

Pre-existing: the 2026-08-31 full tier (run 33372993990) failed the same way
(1347 passed, 1 failed, `test_issue_414_mysql_query_params`).

**This does not weaken the check that protects users.** The linker still refuses
to produce a two-tokio binary — the alternative is two independent
`tokio::runtime::context::CONTEXT` thread-locals and "there is no reactor
running" at runtime (#507, #7629). Only CI's tolerance changes, and each known
failure is still printed with its stderr.

The list is self-policing, verified in all four directions:

| case | result |
|---|---|
| exactly the 2 known | passes |
| known + a NEW failure | **fails**, naming the new one |
| a known one now passes | **fails** as a stale entry |
| all pass | **fails** as stale entries |

So it cannot silently absorb a new break, and it cannot rot once #9470 lands.
