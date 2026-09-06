Quiet Apple M1 diagnostic of the same immutable plan-scan worker and 20 MB
object-root record fixture: 128 stringify calls, 2 warmup calls, a three-second
sample beginning one second after launch. The worker and sampler finish with
exit 0. The separate profile window passes its quiet gate and all 3 external
observations are clean. This does not replace the full CPU/RSS measurements.

Of 2,210 main-thread samples, 1,489 are under the benchmark loop's
`gc_safepoint_moving_minor`, including 1,480 under full mark/sweep. The collector
trace worklist accounts for 1,055 of these samples. Another 702 main-thread
samples are on the stringify call branch; 668 are under js_json_stringify_full.
These are sampled call-stack counts, not a measured GC CPU percentage. They do
not prove exactly how much time a GC optimization would recover.

The local M1 Max diagnostic also finds 1,453 of 2,150 main-thread samples under
the loop GC safepoint, but was taken under high host load and is diagnostic
only. The quiet M1 sample is the stronger evidence for the next investigation.

The large-input cliff needs investigation of collection frequency and tracing
of the live input graph alongside serializer costs. GC production thresholds,
policy and hooks are unchanged by this JSON branch. Do not remove loop polls,
disable GC or exclude collection time to improve the benchmark score.
