Fixed `timer.unref()` being forgotten after 65,536 later timers (#10447).

**What broke.** #6084 bounded the id→ref-state registry at 65,536 entries. It evicted by insertion order and never checked whether an id was still scheduled. A live, long-delay `unref()`'d timer was evicted once 65,536 newer timers had been created. The lookup then read the missing id as ref'd, so the process stayed alive until that timer fired and ran a callback the program had detached. rate-limiter-flexible, which creates one timer per key, lingered 12–15 s after a 1M-operation run. The evicted id also dropped out of `is_known_timer_id`, so `.hasRef()`, `.ref()`, `.unref()`, `.constructor` and `+t` stopped resolving on the handle. The id→kind table (`Timeout`/`Immediate`) had the same cap. Once full, it scanned all 65,536 keys for the minimum on every `setTimeout`, so 200k `clearTimeout(setTimeout(f, 1000))` cost 126 billion instructions.

**Fix.** `crates/perry-runtime/src/timer/ref_states.rs`:
- One registry now holds `has_ref`, `kind` and a `scheduled` flag per id.
- Scheduling returns a `ScheduledTimerId` token that lives in the queue entry itself (`CallbackTimer`, `IntervalTimer`, and the mock-timer entries). Dropping the entry retires the id. Every removal path drops the entry: firing, `clearTimeout`/`clearInterval`/`clearImmediate`, agent purge, and mock clear/reset.
- Only retired ids are eviction candidates, so the map holds at most live timers + 65,536 entries.
- Scheduling does one registry insert, and there is no per-insert scan.
- The whole-queue liveness scans (`ownership.rs`) take the registry lock once per scan instead of once per entry.
- The `timer.rs` filters test `allow_unref` before looking up `has_ref`.
- The registry reports a `timer.ref_states` row in `PERRY_GC_CENSUS`.
- The stale `TIMER_HANDLE_KINDS` entry is removed from `scripts/gc_runtime_root_holders.json`.

**Validation.**
- The gap test `test_gap_10447_timer_ref_state_eviction` matches Node byte for byte. On the baseline, every handle loses its methods and four unref'd callbacks fire.
- New unit tests in `ref_states.rs` cover: scheduled ids surviving churn, live timers beyond the cap, 1M set+clear cycles staying at 65,536 entries, and an end-to-end churn through the real queues.
- Gap suite: 814/820 pass; the 6 failures are the baseline's known ones.
- Instructions: 200k set+clear −99.3 %, 100k chained `setImmediate` −95.1 %. Below-cap schedule/fire/clear workloads are 4–8 % faster, and `asyncpipe` is flat (+0.1 %).
- Registry size is 65,536 entries after both 1M and 3M set+clear cycles.
