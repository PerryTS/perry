// perry/gc — explicit GC control (Perry-native module; does not run under
// Node, so the parity harness records NODE_FAIL and skips it).
import { collect, idleHint, minor } from "perry/gc";

// Churn some garbage so the collections below have work to do.
let sum = 0;
for (let i = 0; i < 200000; i++) {
  const arr = new Array(16).fill(i);
  sum += arr[0] as number;
}
console.log("churned:", sum > 0);

const freed = minor();
console.log("minor returns number:", typeof freed === "number", freed >= 0);

const hinted = idleHint();
console.log("idleHint returns boolean:", typeof hinted === "boolean");

const r = collect();
console.log("collect returns undefined:", r === undefined);

// Still alive and allocating after explicit collections.
const arr = [1, 2, 3].map((x) => x * 2);
console.log("post-gc allocation:", arr.join(","));
console.log("done");
