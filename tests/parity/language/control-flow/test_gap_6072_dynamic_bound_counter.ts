// #6072 repro 2: a `for` counter whose loop bound is a runtime value (not a
// constant) must not be put on a wrapping i32 shadow. Before the fix the
// counter wrapped to INT32_MIN at 2^31 and the loop never terminated.
//
// Every loop below carries a hard iteration cap so a regression fails the
// parity diff instead of hanging the suite.

// `parseInt` keeps the bound opaque to constant folding.
const lim = parseInt("2147483653", 10);

let iters = 0;
const seen: number[] = [];
for (let i = 2147483640; i < lim; i++) {
  iters++;
  if (iters > 50) {
    console.log("BUG: dynamic-bound loop did not terminate");
    break;
  }
  seen.push(i);
}
console.log("cross-2^31 iters:", iters);
console.log("cross-2^31 first/last:", seen[0], seen[seen.length - 1]);

// `i <= n` tops out one past the bound — INT32_MAX itself must not be a fast
// bound, or the last increment overflows.
const atMax = parseInt("2147483647", 10);
let leIters = 0;
let leLast = 0;
for (let i = 2147483645; i <= atMax; i++) {
  leIters++;
  if (leIters > 50) {
    console.log("BUG: <= loop did not terminate");
    break;
  }
  leLast = i;
}
console.log("le iters:", leIters, "last:", leLast);

// A counter seeded past 2^31 with a runtime bound: the entry value itself does
// not fit in an i32, so the guard must reject the fast loop outright.
const hiLim = parseInt("3000000005", 10);
let hiIters = 0;
const hiSeen: number[] = [];
for (let i = 3000000000; i < hiLim; i++) {
  hiIters++;
  if (hiIters > 50) {
    console.log("BUG: high-seed loop did not terminate");
    break;
  }
  hiSeen.push(i);
}
console.log("high-seed iters:", hiIters, "seen:", hiSeen.join(","));

// Ordinary runtime-bounded counters must stay correct (and stay fast).
const n = parseInt("1000", 10);
let sum = 0;
for (let i = 0; i < n; i++) sum += i;
console.log("sum:", sum);

// Runtime-bounded array-index loop: keeps the guarded numeric fast path.
const arr: number[] = new Array(n).fill(0);
for (let i = 0; i < n; i++) arr[i] = i * 2;
let asum = 0;
for (let i = 0; i < n; i++) asum += arr[i];
console.log("asum:", asum, "arr[999]:", arr[999]);

// A constant bound crossing 2^31 was already correct — keep it that way.
let constIters = 0;
for (let i = 2147483640; i < 2147483650; i++) constIters++;
console.log("const-bound iters:", constIters);

// ...and so was `while`.
let w = 2147483640;
let wIters = 0;
while (w < lim) {
  w++;
  wIters++;
  if (wIters > 50) {
    console.log("BUG: while loop did not terminate");
    break;
  }
}
console.log("while iters:", wIters, "w:", w);
