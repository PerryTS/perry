// The holder of an inherited read is reachable ONLY through the receiver's
// prototype link, never through a variable. The read site caches its address.
// Each round a collection moves the holder (forced evacuation), the holder is
// then changed through the live chain, and the SAME site reads again. A site
// that does not root and rewrite its holder reads the old copy and returns the
// value from before the move. Chains of depth 1, 2 and 3 are covered, each
// through its own site; parameter receivers and runtime-length loops keep the
// sites alive (a fixed small loop would be unrolled).
const gcNow: any = (globalThis as any).gc;
function collect(): void {
  const junk: any[] = [];
  for (let i = 0; i < 30000; i++) junk.push({ i, s: "x" + i });
  if (gcNow) gcNow();
}

function read1(o: any): number { const v = o.k; return v === undefined ? -1 : v; }
function read2(o: any): number { const v = o.k; return v === undefined ? -1 : v; }
function read3(o: any): number { const v = o.k; return v === undefined ? -1 : v; }
function hot(f: (o: any) => number, o: any): number {
  let s = 0;
  for (let i = 0; i < 400; i++) s += f(o);
  return s;
}

function holderOf(o: any, depth: number): any {
  let p = o;
  for (let i = 0; i < depth; i++) p = Object.getPrototypeOf(p);
  return p;
}

// each receiver is built so the only reference to its chain is the receiver
function chain(depth: number): any {
  let p: any = { k: 1, tag: "holder" };
  for (let i = 1; i < depth; i++) p = Object.create(p);
  return Object.create(p);
}
const r1 = chain(1);
const r2 = chain(2);
const r3 = chain(3);

console.log("prime", hot(read1, r1), hot(read2, r2), hot(read3, r3));
for (let round = 0; round < 4; round++) {
  collect();
  holderOf(r1, 1).k = 10 + round;
  holderOf(r2, 2).k = 20 + round;
  holderOf(r3, 3).k = 30 + round;
  console.log("round", round, hot(read1, r1), hot(read2, r2), hot(read3, r3));
}

// the same, but the holder is replaced on the chain after the move
collect();
Object.setPrototypeOf(holderOf(r2, 1), { k: 500 });
Object.setPrototypeOf(holderOf(r3, 2), { k: 600 });
console.log("relinked", hot(read2, r2), hot(read3, r3));
collect();
console.log("relinked-after-gc", hot(read2, r2), hot(read3, r3));
