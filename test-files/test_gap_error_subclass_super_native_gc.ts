// `super(message, options)` into a native Error whose heritage is only known at
// run time (the class is built by a factory over a parameter) initializes the
// derived instance in place. The message's ToString and the `cause` getter are
// user code that allocates enough to run evacuating collections; the construct
// must hand back the instance as it is after those collections, not the address
// it was called with, or the derived constructor continues on a stale `this`.
function churn(n) {
  const keep = [];
  for (let i = 0; i < n; i++) {
    const o = { i, s: 'item-' + i };
    if (i % 1000 === 0) keep.push(o);
  }
  return keep.length;
}
function make(Base) {
  return class E extends Base {
    constructor(m, o) {
      super(m, o);
      this.tag = 'ok';
    }
  };
}
const E = make(Error);
const e = new E(
  { toString() { churn(300000); return 'boom'; } },
  { get cause() { churn(300000); return 'c'; } },
);
console.log(e.message, e.tag, e.cause, e instanceof E, e instanceof Error, Object.keys(e));
