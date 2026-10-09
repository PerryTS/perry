// Each record path reenters user code while the record is the sole owner.
// Node runs with --expose-gc; Perry's GC stress also moves on back-edge polls.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
  for (let i = 0; i < 80; i++) { const junk: any = {v: i}; }
}
function bodyCustody(a: any) {
  let sum = 0;
  for (const v of a) {
    collect();
    sum += v.v;
  }
  return sum;
}
function protocolCustody(abrupt: boolean) {
  const input: any = [{v: -1}];
  input[Symbol.iterator] = function() {
    collect();
    return {
      i: 0,
      get next() {
        collect();
        return function() {
          collect();
          const n = ++this.i;
          return {value: {v: n}, done: n > 3};
        };
      },
      return() {
        collect();
        console.log("close", this.i);
        return {};
      }
    };
  };
  let sum = 0;
  try {
    for (const v of input) {
      collect();
      sum += v.v;
      if (sum >= 3) {
        if (abrupt) throw {v: 9};
        break;
      }
    }
  } catch (e: any) { collect(); console.log("error", e.v); }
  return sum;
}
console.log("body", bodyCustody([{v: 1}, {v: 2}, {v: 3}]));
console.log("normal", protocolCustody(false));
console.log("abrupt", protocolCustody(true));
