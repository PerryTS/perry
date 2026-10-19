import { Point, mk, origin, midOnly } from "./exp_star/top.ts";
import { fromA, fromB, fa, fb } from "./exp_star/cyc_a.ts";
import { dup, onlyA, onlyB, same } from "./exp_star/amb_top.ts";
import * as amb2 from "./exp_star/amb_top2.ts";
import { leaf, cyc } from "./exp_star/ns_barrel.ts";
import * as topNs from "./exp_star/top.ts";

// star chain: top -> mid -> leaf
let s = 0;
for (let i = 0; i < 100; i++) { const p = mk(i); p.x += 1; s += p.sum(); }
console.log("chain", s, origin.sum(), midOnly, new Point(3, 4).sum());
console.log("chain-ns", Object.keys(topNs).sort().join(","), "default" in topNs);

// cycle: cyc_a <-> cyc_b
console.log("cycle", fromA, fromB, fa(1), fb(2));

// ambiguity: own export wins over stars; same binding via two stars is one name
console.log("amb", dup, onlyA, onlyB, same);
console.log("amb2", "onlyA" in amb2, "onlyB" in amb2);

// export * as ns
const q = new leaf.Point(5, 6);
q.x = 10;
console.log("ns", q.sum(), leaf.origin.sum(), leaf.mk(2).sum(), cyc.fromA, cyc.fromB, cyc.fb(4));
