// A deferred namespace evaluates its module on the first string-keyed read,
// Object.keys or delete, and never on `then` (#14421). The test that it is a
// deferred namespace runs where the generic paths have classified the
// receiver as a heap object, not on the raw receiver word.
import defer * as a from "./import_defer_ops_a.ts";
import defer * as b from "./import_defer_ops_b.ts";
import defer * as c from "./import_defer_ops_c.ts";

function read(x: any, k: string): unknown {
  return x[k];
}
console.log("start");
console.log(typeof (a as any).then);
console.log("after then");
console.log((a as any).value);
console.log(read(a, "value"), String(read(a, "missing")));
console.log("before keys");
console.log(JSON.stringify(Object.keys(b)));
console.log("before delete");
try {
  console.log(delete (c as any).missing);
} catch (e) {
  console.log((e as Error).name);
}
try {
  console.log(delete (c as any).gone);
} catch (e) {
  console.log((e as Error).name);
}
console.log((c as any).gone);
