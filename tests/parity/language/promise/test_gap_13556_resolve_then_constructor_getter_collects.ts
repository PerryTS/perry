// Gap test for #13556: the fused `Promise.resolve(p).then(f, r)` reads p's own
// `constructor` before it attaches f and r. A getter that allocates enough to
// run a collection must not leave the runtime holding moved handler closures.
// Under PERRY_GC_SCHEDULE_SEED + PERRY_GC_PROTECT_FROMSPACE=1 a stale closure
// faults instead of printing.
function churn(): number {
  let keep = 0;
  for (let i = 0; i < 20000; i++) {
    const cell = { i, next: [i, i + 1] };
    keep += cell.next.length;
  }
  return keep;
}

let reads = 0;
const p = Promise.resolve(7);
Object.defineProperty(p, "constructor", {
  get() {
    reads += churn() > 0 ? 1 : 0;
    return Promise;
  },
});

const label = ["handler", "closures"].join(" ");
Promise.resolve(p).then(
  (v) => console.log(label, "fulfilled", v, reads > 0),
  (e) => console.log(label, "rejected", e),
);
