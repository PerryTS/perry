// Small array literals of statically numeric elements: plain doubles, int32
// boxes, NaN/-0, and annotations that lie, all observed through reads, writes,
// iteration and a collection.

function triple(a: number, b: number, c: number): number[] {
  return [a, b, c];
}
function pair(a: number, b: number): number[] {
  return [a, b];
}

const plain = triple(1.5, -2, 3);
console.log("plain", JSON.stringify(plain), plain.length, plain.indexOf(-2));

const specials = triple(NaN, -0, Infinity);
console.log("specials", specials.map((x) => (Object.is(x, -0) ? "-0" : String(x))).join(","));

const ints = new Set<number>();
for (let i = 0; i < 3; i++) ints.add(i | 0);
const fromSet: number[] = [];
for (const v of ints) fromSet.push(v);
const boxed = triple(fromSet[0], fromSet[1], fromSet[2]);
console.log("boxed", JSON.stringify(boxed), boxed.reduce((s, x) => s + x, 0));

const lying = triple("a" as any, { b: 1 } as any, null as any);
console.log("lying", JSON.stringify(lying), typeof lying[0], typeof lying[1]);

// Numeric writes, a non-numeric write, and growth after construction.
const grown = pair(1, 2);
grown[0] = 10.5;
grown.push(3);
(grown as any)[1] = "two";
grown.push(4.25);
console.log("grown", JSON.stringify(grown));

// Many literals under allocation pressure keep their values.
let total = 0;
const keep: number[][] = [];
for (let i = 0; i < 20000; i++) {
  const lit = triple(i, i * 0.5, -i);
  total += lit[0] + lit[1] + lit[2];
  if (i % 1000 === 0) keep.push(lit);
}
console.log("pressure", total, keep.length, JSON.stringify(keep[5]));
