import { table, nested } from "./dep_a";
import { counters, index, state } from "./dep_b";
import { sums, rx } from "./dep_c";

table[0].name = "z";
nested.inner.k = { fresh: "o" + 1 };
index.set("new", [1, 2]);
index.delete("n1");
counters[0]();
counters[0]();

const keep: unknown[] = [];
let sink = 0;
for (let r = 0; r < 4000; r++) {
  const a: number[] = [];
  for (let j = 0; j < 2000; j++) a.push(j + r);
  sink += a.length;
  if (r % 2 === 0) keep.push({ r, s: "k" + r });
}

function fnv(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h >>> 0;
}
const walk =
  table[0].name + table[199].name + sums[5] + sums[199] + JSON.stringify(nested) +
  index.get("n7") && (index.get("n7") as { id: number }).id +
  "|" + index.get(3) + "|" + JSON.stringify(index.get("new")) + "|" + index.has("n1") + "|" + index.size +
  "|" + counters[0]() + counters[1]() + "|" + keep.length + "|" + sink;
console.log("rx", rx.test("abbc"), "digest", fnv(String(walk)), "inits", state.inits);
