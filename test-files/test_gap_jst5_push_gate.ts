// JST slice 3: the pointer-push gate. A string pushed onto an all-pointers
// array at its append position skips the layout/numeric notes and the shared-
// string demote only when it is already shared. Everything else keeps the full
// sequence. Compared byte-for-byte with Node.

function attempt(f: () => any): string {
  try {
    return String(f());
  } catch (e) {
    return "throw:" + (e instanceof TypeError ? "TypeError" : String(e));
  }
}

// Fresh concat results (shared) appended in a loop.
const built: string[] = [];
for (let i = 0; i < 2000; i++) built.push("item-" + i + ";");
console.log(built.length, built[0], built[1999], built.join("").length);

// A uniquely-owned accumulator pushed and then appended to: the push must
// demote it, or the in-place append rewrites the stored element.
const grown: string[] = [];
let acc = "";
for (let i = 0; i < 300; i++) {
  acc += "q";
  grown.push(acc);
}
let ok = true;
for (let i = 0; i < grown.length; i++) if (grown[i].length !== i + 1) ok = false;
console.log(ok, grown[0], grown[299].length);
let s2 = "ab";
s2 += "cd";
const holder: string[] = [];
holder.push("seed" + 1 + "x");
holder.push(s2);
s2 += "ef";
holder.push(s2);
console.log(holder.join("|"));

// Mixed element kinds on an array that started all-pointers.
const mixed: any[] = [];
mixed.push("a" + 1 + "b");
mixed.push("c" + 2 + "d");
mixed.push(5);
mixed.push("k" + 7);
mixed.push(undefined);
mixed.push({ x: 1 });
mixed.push("e" + 3 + "f");
mixed.push(null);
mixed.push(-0);
mixed.push("");
console.log(mixed.length, JSON.stringify(mixed), Object.is(mixed[8], -0));

// A number[] demoted by a string push.
const nums: any[] = [1, 2, 3];
nums.push("s" + 4 + "t");
nums.push(5);
console.log(nums.join(","), nums.length, typeof nums[3], typeof nums[4]);

// Holey arrays, non-append positions, shrunk and grown lengths.
const holey: string[] = [];
holey.push("a" + 0 + "!");
holey[4] = "far";
holey.push("b" + 1 + "!");
console.log(holey.length, holey.join("|"), 2 in holey, 5 in holey);
const shrunk: string[] = [];
for (let i = 0; i < 10; i++) shrunk.push("v" + i + ";");
shrunk.length = 4;
shrunk.push("w" + 9 + ";");
console.log(shrunk.join(""), shrunk.length);
const grownLen: string[] = [];
grownLen.push("a" + 1 + "b");
grownLen.length = 3;
grownLen.push("c" + 2 + "d");
console.log(grownLen.length, grownLen.join("|"), 1 in grownLen);
const popped: string[] = [];
for (let i = 0; i < 6; i++) popped.push("p" + i + ";");
popped.pop();
popped.shift();
popped.push("n" + 1 + ";");
popped.unshift("u" + 2 + ";");
console.log(popped.join(""));
const sorted: string[] = [];
for (let i = 0; i < 8; i++) sorted.push("z" + ((i * 5) % 8) + ";");
sorted.sort();
sorted.push("last" + 1 + ";");
sorted.splice(2, 1);
sorted.push("after" + 2 + ";");
console.log(sorted.join(""));

// Frozen, sealed and non-extensible arrays.
const frozen: string[] = ["a" + 1 + "b"];
Object.freeze(frozen);
console.log(attempt(() => frozen.push("c" + 2 + "d")), frozen.length);
const sealed: string[] = ["a" + 1 + "b"];
Object.seal(sealed);
console.log(attempt(() => sealed.push("c" + 2 + "d")), sealed.length);
const closed: string[] = ["a" + 1 + "b"];
Object.preventExtensions(closed);
console.log(attempt(() => closed.push("c" + 2 + "d")), closed.length);

// Own properties and non-writable length.
const withProp: string[] = [];
(withProp as any).tag = "t";
for (let i = 0; i < 5; i++) withProp.push("x" + i + ";");
console.log(withProp.length, withProp.join(""), (withProp as any).tag);
const fixedLen: string[] = ["a" + 1 + "b"];
Object.defineProperty(fixedLen, "length", { writable: false });
console.log(attempt(() => fixedLen.push("c" + 2 + "d")), fixedLen.length);

// Inherited setters on the prototype intercept the append.
const writes: string[] = [];
const proto: string[] = [];
for (let i = 0; i < 3; i++) proto.push("m" + i + ";");
Object.defineProperty(Array.prototype, "3", {
  configurable: true,
  set(v: any) {
    writes.push("setter:" + String(v));
  },
  get() {
    return "got";
  },
});
console.log(attempt(() => proto.push("n" + 3 + ";")), proto.length, writes.join("|"), Object.keys(proto).join(","));
delete (Array.prototype as any)[3];
const afterDelete: string[] = [];
for (let i = 0; i < 5; i++) afterDelete.push("d" + i + ";");
console.log(afterDelete.join(""), writes.length);

// A subclass and a proxy receiver.
class Bag extends Array<string> {}
const bag = new Bag();
for (let i = 0; i < 4; i++) bag.push("g" + i + ";");
console.log(bag.length, bag.join(""), bag instanceof Bag);
const proxied = new Proxy([] as string[], {});
for (let i = 0; i < 3; i++) proxied.push("y" + i + ";");
console.log(proxied.length, proxied.join(""));

// Aliased growth: a pushed array element stays intact when the source grows.
let live = "head";
const refs: string[] = [];
for (let i = 0; i < 40; i++) {
  live += "+" + i;
  refs.push(live);
}
console.log(refs[0], refs[1], refs[39].length, refs[20].slice(-6));

// GC pressure: a pointer pushed through the gate must stay traced. Garbage is
// allocated between pushes so that collections run while the strings are held
// only by the arrays (a demoted or never-declared layout that the gate skipped
// would let the collector free or move them unseen).
function churn(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const s = "garbage-" + i + "-" + ((i * 3) % 1000);
    total += s.length;
  }
  return total;
}
const kept: string[] = [];
for (let i = 0; i < 40; i++) {
  kept.push("keep-" + i + ";");
  churn(30000);
}
console.log(kept.length, kept.join(""));
const demoted: any[] = [1, 2, 3];
demoted.push("s" + 4 + "t");
churn(300000);
console.log(demoted.join(","), demoted.length, typeof demoted[3]);
const grownKept: string[] = [];
let accKept = "";
for (let i = 0; i < 40; i++) {
  accKept += "z";
  grownKept.push(accKept);
  churn(20000);
  accKept += "y";
}
let intact = true;
for (let i = 0; i < grownKept.length; i++) if (grownKept[i].length !== 2 * i + 1) intact = false;
console.log(intact, grownKept[0], grownKept[39].length);
