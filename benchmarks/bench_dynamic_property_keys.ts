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
// That ledger was accurate at the time. After the dynamic-write campaigns and
// tombstone deletes became default-on, however, the two columns inverted
// (Mac mini, current main at #9065 filing, min-of-7):
//
//   engine   delete_heavy   overwrite_only   delete penalty
//   node           30 ms           19 ms           1.6x
//   perry         981 ms           13 ms          75.5x
//
// Perry's overwrite loop now beats node; delete-driven shape identity is the
// remaining gap. The ratio that originally argued against dictionary-style
// handling is now the strongest evidence for stable-token, per-key-validated
// churn shapes (#9064/#9065).
//
// Keep BOTH dated ledgers when changing this file. They record a real inversion
// in where the cost lives, not an error in the original measurement.

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
