# Native payload zlib validation (#11919)

This is a working evidence log. It is not a completed acceptance report.
The provider and runtime share one Transform state machine. The stdlib provider,
codec handle tables, callback address queue and zlib root scanner are deleted.
No new production cache or side table is introduced.

## Conversion milestone

Before migration the corrected native handle census is 201 tables and 186
producers (commit `9cfbd61b29` before rebase). After migration it is 199 tables
and 164 producers. The original main census missed two ext-zlib maps; the
separate census correction makes the before/after comparison explicit.

On the initial `899aecd6f9` main baseline, the full release compiler, runtime,
runtime-static, stdlib-static and ext-zlib package set built successfully.
The binding test binary reports 20 passed, one failure, and one ignored test.
The failure is the strengthened 100 MB bomb RSS assertion: default collector
pacing adds about 50 MB RSS, exceeding the test's 32 MiB allowance. A separate
diagnostic run under `PERRY_GC_HEAP_LIMIT=16` passes, with 13,611,008 baseline
bytes and 30,126,080 peak bytes. That diagnostic does not replace the default
acceptance run. The literal 1 KB to 100 MB gzip fixture is not representable;
the valid fixture is roughly 97 KB compressed.

| Witness | Initial result | Sabotage result |
|---|---|---|
| Eleven deferred runtime codecs, release, handle null | PASS | corrupt output / retain handle: RED |
| Brotli destroy in data, immediate native release | PASS | finalizer-only release: RED |
| Six corrupt decoder errors and close | PASS | shared runner error sabotage pending fresh rerun |
| 10,000 one-shots, GC between queue and callback | PASS | raw callback address: RED |
| Bomb bounded input/output and pause | RSS assertion fails under default pacing | oversized scratch: RED; normal RSS failure is unresolved |
| 50,000 Gzip completions plus 50,000 immediate destroys | PASS, 100,000 created = dropped | finalizer-only release: RED |
| Full 50,000 x eleven codec churn | running | pending |

The Gzip-only churn's warmed RSS samples range from 21,917,696 to 22,851,584
bytes, a 933,888 byte spread. Full collection counts still need a diagnostic
companion run.

Pinned Node 26.5.1 output agrees for constructor/prototype/keys/JSON parity,
corrupt-input errors, subclass `_transform`/`_flush`, allocation-pressure
round trips, and the 1 MiB slow-sink pipeline (64 false writes, 64 drains).
Cross-path compressed bytes now agree (1,346 bytes, CRC 2,811,409,573). The
pipe/pipeline trace had one extra final drain; the empty-source EOF correction
is being rebuilt and remains unverified here.

Moving GC round trips agree with Node under seeds 1, 7 and 11919, with forced
evacuation, verification and protected from-space. Their verdicts report
17,022, 17,261 and 17,495 moved objects respectively. This exercises moving
collections rather than merely setting a stress flag.

The initial baseline program binaries agree with Node for tsc
`transpileModule` (three iterations), Zod x5000, qs parse/stringify, commander,
hello, fastify inject and worker_heavy. Effect has matching `ok=20000`; only
its printed construction/decode clock fields differ and are normalized.
buffer_heavy exits 1 on this baseline at its gunzip pipe (#12091). Head output
and interleaved performance measurements are pending.

Buffer layout, native handle census and its sabotage self-test, native ABI,
and pinned Node version checks pass. The file size gate has four unchanged
baseline failures; the modified stream read/write implementation was split
below the 2,000 line cap.

## Current-main integration

Origin advanced to `e87628eb18` with the Buffer B4 conversion. This lane is
being rebased before final build, crate tests, the gap union, and program
measurements. Initial-baseline figures above are not final-main claims.
