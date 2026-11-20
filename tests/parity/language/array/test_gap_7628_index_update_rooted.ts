// #7628: `a[i]++` is a read-modify-write over two operands consumed by four
// calls, each of which can re-enter user code (a getter, a `valueOf`, a
// setter). The operands and the result must be re-read below every one of them.
// Behaviour is unchanged by the rooting repair; this pins the semantics the
// repair must not perturb, on every shape that reaches the generic lowering.

// --- plain array, both fixities --------------------------------------------
const a: number[] = [10, 20, 30];
let i = 0;
console.log(a[i]++, a[0]);
console.log(++a[i], a[0]);
console.log(a[i]--, a[0]);
console.log(--a[i], a[0]);

// --- BigInt elements stay BigInt (#4918) -----------------------------------
const bs: bigint[] = [0n, 5n];
console.log(bs[0]++, bs[0]);
console.log(++bs[1], bs[1]);
console.log(typeof bs[0]);

// --- accessor receiver: the `valueOf` between the read and the write --------
class Cell {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  valueOf(): number {
    return this.n;
  }
}
const cells: Record<string, unknown> = { k: new Cell(7) };
console.log(cells["k" as string]++);
console.log(cells["k"]);

// --- object keys, the lodash `countBy` shape (#957) ------------------------
const counts: Record<string, number> = {};
function key(n: number): string {
  return n % 2 === 0 ? "even" : "odd";
}
for (let n = 0; n < 6; n++) {
  const k = key(n);
  counts[k] = (counts[k] ?? 0) + 0;
  counts[k]++;
}
console.log(JSON.stringify(counts));

// --- property update, both fixities ----------------------------------------
const o: { c: number; b: bigint } = { c: 1, b: 1n };
console.log(o.c++, o.c);
console.log(++o.c, o.c);
console.log(o.b++, o.b);
console.log(++o.b, o.b);

// --- the index expression is evaluated once --------------------------------
let calls = 0;
function idx(): number {
  calls++;
  return 1;
}
const once: number[] = [0, 100];
once[idx()]++;
console.log(once[1], calls);
