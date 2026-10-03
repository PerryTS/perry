// The surviving guard-free numeric load/update sites need independent
// coverage now that P8 routes loop bodies through live shape guards.
// Keep the original loop fixtures unchanged: their floors cover that handoff.
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
