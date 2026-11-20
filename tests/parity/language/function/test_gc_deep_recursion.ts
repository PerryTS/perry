// GC-aggression regression: 800-deep recursion with per-frame
// allocation. Each frame allocates a small object so GC may fire
// mid-recursion. Must NOT crash; result must be exactly correct.
//
// Catches regressions where:
//   - Shadow-stack push/pop (v0.5.218) unbalances during deep
//     recursion (frame_top drifts off the sentinel)
//   - Conservative C-stack scanner misses values in deep frames
//     (saves dead → live transition)
//   - Stack-rooted intermediate `obj` is collected because the
//     scanner doesn't reach the relevant register window
//   - GC-during-recursion produces mid-call inconsistent state
//   - Tenuring of recursion-local objects (Phase C4) traps live
//     objects in OLD_ARENA permanently — RSS bound asserts here

function recurse(n: number): number {
  if (n === 0) return 0;
  // Per-frame allocation — pressures GC during recursion.
  const obj = { n: n, name: "frame_" + n, marker: [n, n + 1, n + 2] };
  // Touch every field so the optimizer can't elide.
  return obj.n + obj.marker[0] - obj.marker[0] + recurse(n - 1);
}

const result = recurse(800);
// Sum of 1..800 = 800 * 801 / 2 = 320400.
console.log("done, result=" + result);
