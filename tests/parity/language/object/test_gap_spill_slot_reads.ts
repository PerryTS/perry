// S5: keys stored in SPILL (overflow) storage are read by the inline shape
// compare. A two-field literal allocates two inline slots, so every key added
// by name afterwards lives in the spill buffer. Receivers are PARAMETERS so the
// reads take the generic property tower.

function grow(o: any, i: number): any {
  o.c = i;
  o.d = "d" + i;
  o.e = i * 2;
  return o;
}

function readD(o: any): any {
  return o.d;
}
function readE(o: any): any {
  return o.e;
}
function readU(o: any): any {
  return o.u;
}

// 1. Monomorphic spill reads.
const objs: any[] = [];
for (let i = 0; i < 64; i++) objs.push(grow({ a: i, b: i }, i));
let acc = 0;
let str = "";
for (let r = 0; r < 200; r++) {
  for (let i = 0; i < objs.length; i++) {
    acc += readE(objs[i]);
    if (r === 199 && i % 16 === 0) str += readD(objs[i]) + ",";
  }
}
console.log("mono", acc, str);

// 2. A spill key holding `undefined` is a value, and it survives the spill
//    buffer growing past its first capacity.
function withUndef(i: number): any {
  const o: any = { a: i, b: i };
  o.c = i;
  o.u = undefined;
  return o;
}
const us: any[] = [];
for (let i = 0; i < 8; i++) us.push(withUndef(i));
let undefSeen = 0;
for (let r = 0; r < 100; r++) for (const o of us) if (readU(o) === undefined && "u" in o) undefSeen++;
console.log("undef", undefSeen);
// Grow one receiver's spill buffer well past 8 entries: its `u` must stay
// `undefined` (not a hole), and still be an own key.
const g = us[3];
for (let k = 0; k < 20; k++) g["k" + k] = k;
console.log("grown", readU(g), "u" in g, Object.keys(g).length, g.k19);
g.u = 42;
console.log("overwritten", readU(g), readU(us[2]));

// 3. A key claimed WITHOUT a value (defineProperty with no `value`) at a spill
//    position, read at the SAME site as data-bearing siblings of the same key
//    list.
function claimed(i: number, how: number): any {
  const o: any = { a: i, b: i };
  o.c = i;
  if (how === 0) o.e = i;
  else if (how === 1) Object.defineProperty(o, "e", { writable: true, enumerable: true, configurable: true });
  else if (how === 2) Object.defineProperty(o, "e", { enumerable: true, configurable: true });
  else Object.defineProperty(o, "e", { get() { return "got" + i; }, enumerable: true, configurable: true });
  return o;
}
const cl: any[] = [];
for (let i = 0; i < 16; i++) cl.push(claimed(i, i % 4));
let claimOut = "";
for (let r = 0; r < 50; r++) {
  for (const o of cl) {
    const v = readE(o);
    if (r === 49) claimOut += String(v) + ";";
  }
}
console.log("claimed", claimOut);
cl[1].e = "late";
cl[5].e = "late5";
console.log("claimed-late", readE(cl[1]), readE(cl[5]), readE(cl[0]), readE(cl[3]));

// 4. A megamorphic site: `d` is spill-located on a dozen shapes.
function shaped(i: number, s: number): any {
  const o: any = { a: i, b: i };
  for (let k = 0; k < s; k++) o["p" + s + "_" + k] = k;
  o.d = s * 1000 + i;
  return o;
}
const mega: any[] = [];
for (let s = 0; s < 12; s++) for (let i = 0; i < 4; i++) mega.push(shaped(i, s));
let megaSum = 0;
for (let r = 0; r < 100; r++) for (const o of mega) megaSum += readD(o);
console.log("mega", megaSum);

// 5. Deleting a spill key moves the receiver off the primed shape.
const del: any = grow({ a: 1, b: 2 }, 7);
for (let r = 0; r < 10; r++) readD(del);
delete del.d;
console.log("deleted", readD(del), readE(del), Object.keys(del).join(","));
del.d = "back";
console.log("readded", readD(del));

// 6. A spill key inherited through the prototype is not an own read.
const proto: any = grow({ a: 0, b: 0 }, 99);
const child: any = Object.create(proto);
child.a = 1;
child.b = 2;
console.log("inherited", readD(child), readE(child));

// 3b. The claimed key is the receiver's FIRST spill key: before the claim it
//     had no spill storage at all. Same read site, same key list as a sibling
//     that wrote the key with a value.
function readC(o: any): any {
  return o.c;
}
function firstSpill(i: number, claim: boolean): any {
  const o: any = { a: i, b: i };
  if (claim) Object.defineProperty(o, "c", { writable: true, enumerable: true, configurable: true });
  else o.c = i;
  return o;
}
const fs: any[] = [];
for (let i = 0; i < 8; i++) fs.push(firstSpill(i, i % 2 === 1));
let firstOut = "";
let undefCount = 0;
for (let r = 0; r < 50; r++) {
  for (const o of fs) {
    const v = readC(o);
    if (v === undefined) undefCount++;
    if (r === 49) firstOut += typeof v + ":" + String(v) + ";";
  }
}
console.log("first-spill", undefCount, firstOut);
let claimedUndef = 0;
for (const o of cl) if (readE(o) === undefined) claimedUndef++;
console.log("claimed-undef", claimedUndef);

// 3c. JSON-born receivers have exactly two inline slots, so a data write and a
//     value-less claim of `c` put it at the same spill position under the SAME
//     shape: the claimed receiver must carry storage for it, or the read the
//     data sibling primed would load through a receiver with none.
function claimC(o: any): any {
  Object.defineProperty(o, "c", { writable: true, enumerable: true, configurable: true });
  return o;
}
function writeC(o: any, i: number): any {
  o.c = i;
  return o;
}
const js: any[] = [];
for (let i = 0; i < 8; i++) {
  const born = JSON.parse('{"a":1,"b":2}');
  js.push(i % 2 === 1 ? claimC(born) : writeC(born, i));
}
let jsUndef = 0;
let jsSum = 0;
for (let r = 0; r < 50; r++) {
  for (const o of js) {
    const v = readC(o);
    if (v === undefined) jsUndef++;
    else jsSum += v;
  }
}
console.log("json-claimed", jsUndef, jsSum, Object.keys(js[1]).join(","), "c" in js[1]);
