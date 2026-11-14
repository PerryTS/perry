// family_reach with PERRY_FACTS_FAMILY_LINK=full: the byte-cell family is
// "live" although the program text holds no typed-array name in one piece
// (the constructors are reached through constant computed keys of the global
// object, static keys to the fact). In that mode the install follows the
// fact, so the typed-array group is linked and the method tower answers.
// (The text detector sees no "Uint8Array" here and leaves the group out.)
const g: any = globalThis;
const U8 = g["Uint8" + "Array"];
const F64 = g["Float64" + "Array"];
const I32 = g["Int32" + "Array"];

const a: any = new U8(6);
for (let i = 0; i < 6; i++) a[i] = (i * 50) % 256;
console.log("u8", a.length, a[0], a[5], a.at(-1), a.indexOf(100), a.includes(150), a.lastIndexOf(250));
console.log("join", a.join("-"), String(a.slice(1, 3)), String(a.subarray(4)));
console.log("iter", JSON.stringify([...a.keys()]), JSON.stringify([...a.values()]), JSON.stringify([...a.entries()][2]));

const f: any = new F64([3.5, -1, 2, 0.25]);
console.log("f64", f.length, String(f.toSorted()), String(f.toReversed()), String(f.with(1, 9)), String(f));
f.sort();
console.log("sorted", String(f), f.find((v: number) => v > 1), f.findIndex((v: number) => v > 1), f.findLast((v: number) => v < 1), f.findLastIndex((v: number) => v > 100));
console.log("fold", f.reduce((s: number, v: number) => s + v, 0), f.reduceRight((s: string, v: number) => s + v, ""), f.some((v: number) => v < 0), f.every((v: number) => v < 0));
console.log("map", String(f.map((v: number) => v * 2)), String(f.filter((v: number) => v > 0)));
let seen = "";
f.forEach((v: number, i: number) => { seen += i + ":" + v + ";"; });
console.log("each", seen);

const n: any = new I32(5);
n.fill(7, 1, 4);
n.set([1, 2], 3);
n.copyWithin(0, 3);
n.reverse();
console.log("i32", String(n), n.toLocaleString(), n.byteLength, n.BYTES_PER_ELEMENT);

// The method tower's argument checks.
for (const bad of [() => f.map(1), () => f.forEach(undefined), () => f.sort("x"), () => f.reduce(null)]) {
  try { bad(); console.log("no throw"); } catch (e) { console.log((e as Error).constructor.name); }
}

// Negative controls: an ordinary object and an Array with the same calls.
const plain: any = { at: () => "own at", length: 2 };
console.log(plain.at(0), [1, 2, 3].at(0), typeof g["Uint8" + "Arr"]);
