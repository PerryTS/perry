// Charter step 5: every writer of an F64 slot runs the store check.
// A key-add of a Number gives the new shape an F64 lane. Each section below
// owns its keys and its store sites, so no earlier section has deprecated the
// lane it relies on, and each ends with a full collection, which traces every
// live object: a runtime built with `field-rep-assert` checks each F64 lane at
// every trace, so a non-Number stored raw under an F64 lane aborts the run.
//   1. the emitted store IC hit on an F64 lane is served a non-Number;
//   2. the emitted key-add memo, learned from a Number, is served a non-Number;
//   3. the inline arm is served NaN / Infinity / -0 (the funnel canonicalizes);
//   4. delete moves a string down into the slot of an F64 lane;
//   5. class-instance fields (class-field guard, constructor prologue) are
//      learned from Numbers, then served non-Numbers;
//   6. a class instance grows past its declared fields by a Number key-add,
//      then its declared field is stored through the class-field guard.
declare function gc(): void;
function collect(): void {
  if (typeof gc === "function") gc();
}
function fresh(): any {
  return {};
}

// 1. Existing-key store IC hit.
function addIa(o: any, v: any): void {
  o.ia = v;
}
function setIa(o: any, v: any): void {
  o.ia = v;
}
function icHit(): string {
  const keep: any[] = [];
  for (let i = 0; i < 200; i++) {
    const o = fresh();
    addIa(o, i + 0.5);
    keep.push(o);
  }
  // ONE loop, so the site (even an inlined copy) is primed on the F64 shape
  // by the Numbers and then hit with the first non-Number.
  for (let i = 0; i < 400; i++) {
    const v = i < 200 ? i * 2 + 0.25 : i % 2 === 0 ? "s" + i : { n: i };
    setIa(keep[i % 200], v);
  }
  collect();
  let s = 0;
  let n = 0;
  for (const o of keep) {
    if (typeof o.ia === "string") s++;
    else n += o.ia.n;
  }
  return s + " " + n;
}

// 2. Key-add memo hit.
function addKb(o: any, v: any): void {
  o.kb = v;
}
function keyAdd(): string {
  const added: any[] = [];
  // ONE loop: the memo is learned from Numbers, then served non-Numbers.
  for (let i = 0; i < 400; i++) {
    const o = fresh();
    addKb(o, i < 200 ? i + 1.5 : i % 2 === 0 ? "t" + i : [i]);
    added.push(o);
  }
  collect();
  let s = 0;
  let a = 0;
  let sum = 0;
  for (const o of added) {
    if (typeof o.kb === "string") s++;
    else if (Array.isArray(o.kb)) a += o.kb[0];
    else sum += o.kb;
  }
  return s + " " + a + " " + sum;
}

// 3. Doubles the inline arm must not store as is.
function addSc(o: any, v: any): void {
  o.sc = v;
}
function setSc(o: any, v: any): void {
  o.sc = v;
}
function specials(): string {
  const special: any[] = [];
  for (let i = 0; i < 300; i++) {
    const o = fresh();
    addSc(o, 1.5);
    special.push(o);
  }
  for (let i = 0; i < 600; i++) {
    const v = i < 300 ? 2.5 : i % 3 === 0 ? NaN : i % 3 === 1 ? Infinity : -0;
    setSc(special[i % 300], v);
  }
  collect();
  let nan = 0;
  let inf = 0;
  let negz = 0;
  for (const o of special) {
    if (Number.isNaN(o.sc)) nan++;
    else if (o.sc === Infinity) inf++;
    else if (Object.is(o.sc, -0)) negz++;
  }
  return nan + " " + inf + " " + negz;
}

// 4. Delete shifts a string down into the slot of an F64 lane.
function addDa(o: any, v: any): void {
  o.da = v;
}
function addDs(o: any, v: any): void {
  o.ds = v;
}
function addDz(o: any, v: any): void {
  o.dz = v;
}
function deletes(): string {
  const del: any[] = [];
  for (let i = 0; i < 300; i++) {
    const o = fresh();
    addDa(o, i + 0.5);
    addDs(o, "str" + i);
    addDz(o, i + 1.5);
    delete o.da;
    del.push(o);
  }
  collect();
  let ds = 0;
  let dz = 0;
  for (const o of del) {
    if (typeof o.ds === "string" && o.ds.startsWith("str")) ds++;
    dz += o.dz;
    if ("da" in o) ds = -1;
  }
  return ds + " " + dz + " " + Object.keys(del[7]).join(",");
}

// 5. Class instances: constructor-prologue stores and class-field stores.
class Pt {
  cx: any;
  cy: any;
  constructor(x: any, y: any) {
    this.cx = x;
    this.cy = y;
  }
}
function setCx(p: Pt, v: any): void {
  p.cx = v;
}
function classes(): string {
  const pts: Pt[] = [];
  for (let i = 0; i < 400; i++) {
    const x = i < 200 ? i + 0.5 : i % 2 === 0 ? "d" + i : [i];
    pts.push(new Pt(x, i + 0.25));
  }
  for (let i = 0; i < 400; i++) {
    const v = i < 200 ? i + 0.75 : i % 2 === 0 ? "c" + i : { n: i };
    setCx(pts[i % 200], v);
  }
  collect();
  let s = 0;
  let n = 0;
  let sum = 0;
  for (const p of pts) {
    if (typeof p.cx === "string") s++;
    else if (Array.isArray(p.cx)) n += p.cx[0];
    else if (typeof p.cx === "object") n += p.cx.n;
    else sum += p.cx;
    sum += p.cy;
  }
  return s + " " + n + " " + sum;
}

// 6. A class instance's key-add past its declared fields, then class-keyed
//    stores of its declared field on the grown instance.
class Bag {
  bx: any;
  constructor(x: any) {
    this.bx = x;
  }
}
function addExtra(o: any, v: any): void {
  o.extra = v;
}
function setBx(b: Bag, v: any): void {
  b.bx = v;
}
function grown(): string {
  const bags: Bag[] = [];
  for (let i = 0; i < 400; i++) {
    const b = new Bag(i + 0.5);
    addExtra(b, i < 200 ? i + 0.25 : "e" + i);
    bags.push(b);
  }
  for (let i = 0; i < 800; i++) {
    const v = i < 400 ? i + 0.75 : i % 2 === 0 ? "b" + i : { n: i };
    setBx(bags[i % 400], v);
  }
  collect();
  let s = 0;
  let n = 0;
  let sum = 0;
  for (const b of bags) {
    const e: any = (b as any).extra;
    if (typeof e === "string") s++;
    else sum += e;
    if (typeof b.bx === "string") s++;
    else n += b.bx.n;
  }
  return s + " " + n + " " + sum;
}

console.log(icHit());
console.log(keyAdd());
console.log(specials());
console.log(deletes());
console.log(classes());
console.log(grown());
