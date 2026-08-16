`node:perf_hooks` — implemented the histogram surface and the dispatch bug hiding behind it, taking the module's node-suite from **86/148 to 132/148** (46 tests, no regressions; issue #6766).

**Histograms are real.** `createHistogram()` and `monitorEventLoopDelay()` returned a stub whose every stat read `0` and whose `record`/`recordDelta`/`add`/`reset` discarded input, so two handles were indistinguishable. Both are now backed by an HDR histogram (`crates/perry-runtime/src/perf_histogram.rs`), a port of the parts of `hdr_histogram.c` Node's accessors actually reach. The bucketing is not an implementation detail: `percentile(1) === min` and `percentile(100) === max` hold only because both runtimes quantize identically, and the `percentiles` Map's keys (`0,50,75,100` for two samples) come out of hdr's percentile iterator rather than the sample list. Nine unit tests pin values captured from the Node 26.5.1 oracle. Every stat and its BigInt twin, `toJSON()`, the Number/BigInt `record` pair, `recordDelta()`, `add()` isolation, the ELD `enable`/`disable` lifecycle and Node's `validateInteger`/`validateObject` error codes now match; `timerify(fn, { histogram })` validates the handle and records call durations into it in nanoseconds.

**Method calls on the internal `perf_*` namespaces did nothing at all.** `perf_histogram`, `perf_observer` and `perf_observer_list` are namespace tags that can never appear in user source — they are handed out as return values. Codegen emits a module's `js_nm_install_perf()` only where it sees the module *named*, so the dispatch bucket for these three stayed empty and every method call on such an object resolved to `undefined` and silently no-op'd. That is why the old stub and the new histogram behaved identically, and why `list.getEntries()` returned `undefined` inside an observer callback. The bucket is now armed where the objects are minted. A second, independent loss of the receiver is fixed alongside it: `h.record(5)` lowers as a value read plus an indirect call, and the value read minted a *module-level* bound closure capturing a freshly-created namespace — which has no instance id.

Also fixed, each pinned by a case in `test-parity/node-suite/perf_hooks/`:

- `perf_hooks.constants` shared the `perf_hooks` namespace tag, so `Object.keys(constants)` enumerated the module's export list instead of the `NODE_PERFORMANCE_GC_*` table. It has its own tag now, and gains the missing `NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP`.
- PerformanceObserver callbacks were dispatched from a `setTimeout(0)` — the timer phase. Node dispatches from the check phase, so a caller that created an entry and then awaited one `setImmediate` saw "not delivered". They also now run with `this` bound to the observer, and `takeRecords()` no longer swallows the already-queued callback.
- `observe()` that resolves to no supported entry type is a no-op and no longer pins the subscription mode; switching mode on an active observer raises `InvalidModificationError`, and `observe({ type })` accumulates rather than replacing.
- `markResourceTiming()` produced a 5-field entry with a `NaN` duration. It now projects the full `PerformanceResourceTiming` field set (timings, body sizes, `transferSize` with the fetch spec's 300-byte allowance, `responseStatus`, `deliveryType`), serializes all 23 keys through `toJSON()`, and validates `cacheMode`.
- `performance.nodeTiming` is a single cached instance (so `timing === performance.nodeTiming` and the `toJSON()` snapshot's non-freshness both hold), gains a `toJSON()`, and reports Node's `loopStart` "not started" sentinel — the gate `eventLoopUtilization()` reads before reporting anything but zeros.
- `PerformanceObserver.supportedEntryTypes` returns Node's full list, frozen, as the same array on every read.
- `mark()`/`clearMarks()` reject Node's reserved bootstrap-milestone names, `measure()` resolves those names against `nodeTiming`, an unset positional mark endpoint raises `SyntaxError`, and the `getEntriesBy*` queries take Node's missing-argument and Symbol guards.
- `eventLoopUtilization` was marked internal in the API manifest, so `import { eventLoopUtilization } from "node:perf_hooks"` — valid in Node — was rejected at compile time.

Gate follow-up: the two new thread-locals (`HISTOGRAMS` in `perf_histogram.rs`,
`RESOURCE_TIMING_BUFFER_SIZE` in `perf_hooks/resource_timing.rs`) now use
`crate::perry_thread_local!` rather than a raw `thread_local!`, so neither
needs a cold-allowlist exemption and both skip the Darwin `_tlv_get_addr`
call (#7469). The cold ratchet drops from 91 recorded files back to 90.

The four new raw `keys_array` reads are recorded in the shape-descriptor
census baseline. They are identity indices, not owners: all three cells
(`PERF_ENTRY_KEYS_ARRAY`, `RESOURCE_ENTRY_KEYS_ARRAY`, `NODE_TIMING_KEYS_ARRAY`)
are visited by the registered root scanner via `visit_metadata_usize_slot`,
which follows a forwarding address without keeping the keys array alive.
