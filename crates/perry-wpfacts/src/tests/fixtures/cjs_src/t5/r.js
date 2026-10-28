const { B } = require("./b");
const NONE = null;
class P {
  constructor(v) { this.v = v; this.link = NONE; }
  get() { return this.v; }
}
class S {
  constructor(n) { function z(y) { return y; } z.call(this, n); this.p = new P(n); }
}
class Q {
  constructor(n) { this.q = new P(n); }
}
function mk(n) { return new Q(n); }
class T extends B {
  constructor() { super(); this.k = NONE; }
  run() { function w(y) { return y; } w.call(this, 0); const s = new S(1); const q = mk(2); return s.p.get() + q.q.get() + s.p.v + q.q.v; }
}
exports.newInstance = () => new T();
