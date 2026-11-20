// Stores of YOUNG values into OLD objects through the static-key store IC.
// The parents are promoted by churn before the stores begin; each store's
// child is reachable ONLY through its old parent while later minor
// collections run. The inline hit must record the old->young edge in the
// remembered set (the write barrier behind the TENURED/incremental gate): a
// minor that does not know the edge frees or moves the child without
// rewriting the slot. `PERRY_GC_VERIFY_EVACUATION=1` checks every old->young
// edge against the remembered set at each minor and aborts on a missing one.

function churn(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const tmp = { a: i, b: [i, i + 1], s: "t" + i };
    t += tmp.b.length + tmp.s.length;
  }
  return t;
}

function link(p: any, c: any): void {
  p.child = c;
}
function tag(p: any, s: string): void {
  p.label = s;
}

const parents: any[] = [];
for (let i = 0; i < 300; i++) parents.push({ id: i, child: null, label: "", n: 0 });
churn(400000); // promote the parents

let checks = 0;
for (let round = 0; round < 40; round++) {
  for (let i = 0; i < parents.length; i++) {
    link(parents[i], { v: round * 1000 + i, s: "c" + round + "_" + i });
    tag(parents[i], "L" + round + "." + i);
  }
  churn(15000); // minors while the children hang off old parents only
  for (let i = 0; i < parents.length; i += 37) {
    const p = parents[i];
    if (p.child.v !== round * 1000 + i || p.child.s !== "c" + round + "_" + i || p.label !== "L" + round + "." + i) {
      console.log("LOST", round, i, JSON.stringify(p));
      break;
    }
    checks++;
  }
}
churn(100000);
let sum = 0;
for (const p of parents) sum += p.child.v + p.label.length;
console.log("checks", checks, "sum", sum);
