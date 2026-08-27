// Benchmark: dynamic string-keyed property access, with and without `delete`.
//
// Two loops do the SAME number of property writes and reads; only the `delete`
// differs. Comparing them isolates shape churn from raw property-access cost,
// which is what makes this benchmark worth keeping:
//
//   * `deleteHeavy / overwriteOnly` is the *delete penalty* — how much a
//     delete-driven shape walk costs relative to a stable shape.
//   * `overwriteOnly` on its own is *baseline dynamic property throughput*.
//
// Measured 2026-08-28 (idle host, perry 0.5.1519, N = 300_000):
//
//   engine   delete_heavy   overwrite_only   delete penalty
//   node           36 ms           21 ms           1.7x
//   perry        1487 ms         1321 ms           1.1x
//
// The delete penalty is the number the "objects that defeat shapes need a
// dictionary mode" argument rests on — and perry's is LOWER than node's. Adding
// a dictionary representation would therefore be a large investment aimed at a
// tail perry does not have.
//
// The second column is the real gap: ~60x on plain overwrite. Profiling this
// binary puts the time in `js_array_get_f64`, `try_read_tracked_gc_header`,
// `shape_descriptor_by_id` + `shape_descriptor_ensure_with_generation` (two
// hash lookups per access on the hot path), and `js_put_value_set_dyn_ic_miss`
// — i.e. inline-cache misses and shape-table probes, not deletion.
//
// Keep both columns when changing this file: the ratio is what refutes the
// dictionary-mode premise, and the absolute is what tracks the real gap.

function deleteHeavy(n: number): number {
  const o: Record<string, number> = {};
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = "k" + (i % 500);
    o[k] = i;
    s += o[k];
    delete o[k]; // walks the object back to a previous key set
  }
  return s;
}

function overwriteOnly(n: number): number {
  const o: Record<string, number> = {};
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = "k" + (i % 500);
    o[k] = i;
    s += o[k]; // same writes; the shape stabilises after 500 keys
  }
  return s;
}

const N = 300000;

let t = Date.now();
const a = deleteHeavy(N);
const deleteMs = Date.now() - t;

t = Date.now();
const b = overwriteOnly(N);
const overwriteMs = Date.now() - t;

console.log(
  "delete_heavy_ms=" +
    deleteMs +
    " overwrite_ms=" +
    overwriteMs +
    " delete_penalty=" +
    (deleteMs / Math.max(overwriteMs, 1)).toFixed(1) +
    "x checksum=" +
    ((a + b) % 7),
);
