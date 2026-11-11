// A statement-form compound member assignment through a `const` binding reads
// and writes the binding itself; a mutable binding keeps its evaluated-once
// snapshot. Each case below distinguishes the two.
const log: string[] = [];

// 1. const receiver, RHS allocates enough to collect: the write must land on
//    the same (possibly moved) object.
class Acc { total = 0; label = ""; }
const acc = new Acc();
for (let i = 0; i < 2000; i++) {
  acc.total += [i, i + 1, i + 2].map((x) => x * 2).reduce((a, b) => a + b, 0);
  acc.label += i % 500 === 0 ? String(i) + "," : "";
}
console.log("acc", acc.total, acc.label);

// 2. mutable receiver reassigned inside the RHS: the OLD object is written.
let o = { x: 1 };
const p = { x: 10 };
const first = o;
o.x += ((o = p), 5);
console.log("mut", first.x, p.x, o === p);

// 3. const receiver with a computed key that has side effects: base before key.
const arr = [1, 2, 3];
let k = 0;
const nextKey = () => (log.push("key" + k), k++);
arr[nextKey()] += 100;
arr[nextKey()] *= 7;
console.log("keyed", JSON.stringify(arr), log.join(","));

// 4. const receiver and const key: both read straight from the bindings.
const table: Record<string, number> = { a: 1, b: 2 };
const ka = "a";
table[ka] += 41;
table[ka] -= 2;
console.log("constkey", JSON.stringify(table));

// 5. accessors see the right receiver, once each per compound assignment.
const target = {
  _v: 1,
  get v() { log.push("get"); return this._v; },
  set v(x: number) { log.push("set:" + x); this._v = x; },
};
log.length = 0;
target.v += 1;
target.v ||= 9;
target.v &&= 4;
console.log("accessor", target._v, log.join(","));

// 6. mixed number/object fields under churn.
class Body { x = 0.5; vx = 1.25; tag = "b"; }
const bodies = [new Body(), new Body(), new Body()];
let sum = 0;
for (let s = 0; s < 3000; s++) {
  for (let i = 0; i < bodies.length; i++) {
    const b = bodies[i];
    b.vx -= b.x * 0.001;
    b.x += b.vx * 0.01;
    b.tag += s % 1000 === 0 ? "+" : "";
    if (s % 100 === 0) { const junk = new Array(200).fill({ s }); sum += junk.length; }
  }
}
console.log("bodies", bodies.map((b) => b.x.toFixed(6) + "/" + b.tag).join(" "), sum);

// 7. a mutable receiver reassigned by its own RHS, through class fields,
//    arrays and a loop: every write lands on the object read first.
class Cell { total = 0; label = ""; }
function reassignInLoop(): string {
  let c = new Cell();
  const other = new Cell();
  const firstCell = c;
  for (let i = 0; i < 3; i++) { c.total += ((c = i % 2 ? firstCell : other), 5); }
  return firstCell.total + "/" + other.total;
}
function reassignString(): string {
  let c = new Cell();
  const other = new Cell();
  const firstCell = c;
  c.label += ((c = other), "x");
  return JSON.stringify(firstCell) + JSON.stringify(other);
}
function reassignArray(): string {
  let a = [1, 2, 3];
  const b = [10, 20, 30];
  const firstArr = a;
  a[1] += ((a = b), 100);
  return JSON.stringify(firstArr) + JSON.stringify(b);
}
console.log("reassign", reassignInLoop(), reassignString(), reassignArray());
