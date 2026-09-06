Branch: codex/json-fastpaths-1520. Draft PR: https://github.com/PerryTS/perry/pull/9849.
Measured JSON source: 7117f9e67d11af140ee857880b9a96af2ed2890e.
Worker SHA256: 603487b666e334940ea399ab029ced7745673239ca93241dbab8dc8f5d4f4fb2.

No production GC-directory changes versus v0.5.1520 (454daac4f8fc667ab4bc85b7b5b36c8bae56ae28).
No GC threshold, gc_bump_malloc_trigger or parse-boundary policy changes. JSON
input-root ordering and rederivation fixes remain part of this branch; existing
hooks are preserved. JSON output plans contain native counts and inline text,
and final output allocation rederives input pointers from roots.

The quiet M1 20 MB stringify profile has 1,489/2,210 main-thread samples under
the benchmark loop GC safepoint and 1,480 under full mark/sweep; 668 are under
js_json_stringify_full. This is call-stack sampling, not an exact CPU percentage.
The raw profile, metadata and quiet gate are in large-record-profile/. The GC
trace worklist dominates the GC branch in this sample. Investigate why this
workload repeatedly traces its live input graph and whether the separate GC
work removes that cost. JSON changes must preserve input/output liveness and
must not suppress the benchmark's collection work or change its call counts.
