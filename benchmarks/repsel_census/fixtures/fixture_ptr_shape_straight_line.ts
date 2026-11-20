// The surviving guard-free numeric load/update sites need independent
// coverage now that P8 routes loop bodies through live shape guards.
// Keep the original loop fixtures unchanged: their floors cover that handoff.
//
// Whole-program facts lower the same accesses without a guard whenever they
// prove the receiver (`expr/wpfacts_access.rs` runs before the Ptr<Shape>
// arms), so `c` stays exercised only where the facts decline. The runtime
// `new Function` at the bottom is that gap: once interpreted code can run
// (decision 150, `interp_guard`), facts keep reads on a guarded route and
// leave stores, updates and calls to their checked routes, while `c` is still
// bound once by `new StraightCounter()` and Ptr<Shape> proves it. Without the
// dynamic site every access here is a facts access and this fixture counts 0
// consumed.
class StraightCounter {
  v: number;
  w: number;
  constructor() {
    this.v = 0;
    this.w = 1;
  }
  mix(): number {
    return this.v * this.v + this.w * this.w;
  }
}

function straightLine(): number {
  const c = new StraightCounter();
  // A self-reading store prevents scalar replacement. There is deliberately
  // no loop, so region handoff cannot consume these accesses instead.
  c.v = c.v + 1;
  c.w++;
  return c.v * c.w + c.mix();
}

console.log("ptr_shape_straight_line:" + straightLine());

// The facts gap (see the header): source built at runtime, so it is not
// compiled statically and the program can run interpreted code.
const dynamicCode = new Function("return " + process.argv.length);
console.log("dynamic:" + typeof dynamicCode());
