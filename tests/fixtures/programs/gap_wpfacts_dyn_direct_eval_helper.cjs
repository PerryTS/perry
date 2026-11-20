// Helper of test_gap_wpfacts_dyn_direct_eval.cjs: `Box` lives here, out of
// the direct eval's lexical reach (it can name only its own module).
class Box {
  constructor(n) {
    this.n = n;
  }
}

function readN(b) {
  return b.n;
}

function total() {
  let t = 0;
  for (let i = 0; i < 3; i++) t += readN(new Box(i + 1));
  return t;
}

module.exports = { readN, total };
