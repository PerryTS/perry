// Gap test for #13556: when p has an own `then`, the fused
// `Promise.resolve(p).then(f, r)` reads it and calls what it returns with p as
// `this` and f, r as arguments. A getter that allocates enough to run a
// collection must not leave the runtime holding a moved receiver or moved
// handler closures. Under PERRY_GC_SCHEDULE_SEED + PERRY_GC_PROTECT_FROMSPACE=1
// a stale value faults instead of printing.
function churn(): number {
  let keep = 0;
  for (let i = 0; i < 20000; i++) {
    const cell = { i, next: [i, i + 1] };
    keep += cell.next.length;
  }
  return keep;
}

let reads = 0;
const realThen = Promise.prototype.then;
const p = Promise.resolve(7);
Object.defineProperty(p, "then", {
  get() {
    reads += churn() > 0 ? 1 : 0;
    return function (this: Promise<number>, f: any, r: any) {
      return realThen.call(this, f, r);
    };
  },
});

const label = ["handler", "closures"].join(" ");
Promise.resolve(p).then(
  (v) => console.log(label, "fulfilled", v, reads),
  (e) => console.log(label, "rejected", e),
);
