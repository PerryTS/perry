// #7574 — `class X extends Array` held in a `T[]`-annotated binding took the
// raw `ArrayHeader` fast paths. An Array-subclass instance is a plain
// `ObjectHeader`, and the two headers overlay field for field, so the element
// slots at +8/+16/+24 are `parent_class_id ‖ field_count`, `keys_array` and
// `meta`. Element writes overwrote two live GC child edges: `a.push(1);
// a.push(2)` SIGSEGVed (exit 139) on the SECOND push, with zero output.
//
// A declared TypeScript type is a hint, never a layout fact, so every binding
// form is affected: `const`, parameter, class field, return type, `as` cast.
// Sibling of #7570 (Map/Set, fixed by #7573).
//
// KNOWN GAP, deliberately not asserted here: `ArraySpeciesCreate` on a subclass
// — node's `sub.map(f)` returns a `MyArr`, perry returns a plain `Array`. That
// is pre-existing and identical on the UNANNOTATED path, so this file compares
// element CONTENT (via `join`) rather than the container's console formatting.

class MyArr<T> extends Array<T> {}

class Indirect<T> extends MyArr<T> {}

class WithCtorAndFields extends Array<number> {
  tag = "wcf";
  constructor() {
    super();
  }
}

function useParam(p: number[]): string {
  p.push(1);
  p.push(2);
  p[0] = 9;
  return `param len=${p.length} p0=${p[0]} p1=${p[1]}`;
}

function makeArr(): number[] {
  const r: number[] = new MyArr<number>();
  r.push(7);
  return r;
}

class Holder {
  items: number[] = new MyArr<number>();
}

function seed(n: number): number[] {
  const a: number[] = new MyArr<number>();
  for (let i = 0; i < n; i++) {
    a.push((i + 1) * 10);
  }
  return a;
}

// ---------------------------------------------------------------------------
// 1. The crash repro: two pushes through a base-typed const binding.
// ---------------------------------------------------------------------------
const crashRepro: number[] = new MyArr<number>();
crashRepro.push(1);
console.log("push1", crashRepro.length);
crashRepro.push(2);
console.log("push2", crashRepro.length, crashRepro[0], crashRepro[1]);

// ---------------------------------------------------------------------------
// 2. Every binding form.
// ---------------------------------------------------------------------------
const asConst: number[] = new MyArr<number>();
asConst.push(1);
asConst.push(2);
console.log("const", asConst.length, asConst[0], asConst[1]);

console.log(useParam(new MyArr<number>()));

const fromReturn = makeArr();
fromReturn.push(8);
console.log("return", fromReturn.length, fromReturn[0], fromReturn[1]);

const holder = new Holder();
holder.items.push(5);
holder.items.push(6);
console.log("field", holder.items.length, holder.items[0], holder.items[1]);

const asCast = new MyArr<number>() as number[];
asCast.push(3);
console.log("cast", asCast.length, asCast[0]);

const indirect: number[] = new Indirect<number>();
indirect.push(4);
console.log("indirect", indirect.length, indirect[0]);

const withFields: number[] = new WithCtorAndFields();
withFields.push(11);
console.log("ctor+fields", withFields.length, withFields[0]);

// ---------------------------------------------------------------------------
// 3. Element get / set through the annotated binding.
// ---------------------------------------------------------------------------
const idx: number[] = new MyArr<number>();
idx[0] = 10;
console.log("set0", idx.length, idx[0]);
idx[1] = 20;
console.log("set1", idx.length, idx[1]);
idx[0] = 99;
console.log("overwrite", idx.length, idx[0], idx[1]);
console.log("oob", idx[7]);

// ---------------------------------------------------------------------------
// 4. `.length` READ and WRITE.
// ---------------------------------------------------------------------------
const lenRW: number[] = seed(3);
console.log("len read", lenRW.length);
lenRW.length = 1;
console.log("len write", lenRW.length, lenRW[0], lenRW[1], lenRW[2]);
lenRW.length = 0;
console.log("len zero", lenRW.length, lenRW[0]);

// ---------------------------------------------------------------------------
// 5. push / pop / shift.
// ---------------------------------------------------------------------------
const mut: number[] = seed(3);
console.log("pop", mut.pop(), mut.length);
console.log("shift", mut.shift(), mut.length);
mut.push(77);
console.log("push back", mut.length, mut[0], mut[1]);

// ---------------------------------------------------------------------------
// 6. The bounded-index loop tier (hoisted `arr.length` + raw slot load).
// ---------------------------------------------------------------------------
const looped: number[] = seed(4);
let boundedSum = 0;
for (let i = 0; i < looped.length; i++) {
  boundedSum += looped[i];
}
console.log("bounded sum", boundedSum);

// ---------------------------------------------------------------------------
// 7. Iteration + spread.
// ---------------------------------------------------------------------------
const iterated: number[] = seed(3);
const forOf: number[] = [];
for (const v of iterated) {
  forOf.push(v);
}
console.log("for-of", forOf.join(","));
console.log("spread", [...iterated].join(","));
console.log("Array.from", Array.from(iterated).join(","));
const [d0, d1] = iterated;
console.log("destructure", d0, d1);

// ---------------------------------------------------------------------------
// 8. map / filter / forEach receiver identity / join / slice / indexOf.
// ---------------------------------------------------------------------------
const funcs: number[] = seed(3);
console.log("map", funcs.map((v) => v * 2).join(","));
console.log("filter", funcs.filter((v) => v > 10).join(","));
console.log("slice", funcs.slice(1).join(","));
console.log("join", funcs.join("-"));
console.log("indexOf", funcs.indexOf(20), funcs.indexOf(999));
console.log("includes", funcs.includes(30), funcs.includes(999));
console.log("reduce", funcs.reduce((a, b) => a + b, 0));
funcs.forEach(function (v, i, self) {
  console.log("forEach", i, v, self === funcs, self.length);
});

// ---------------------------------------------------------------------------
// 9. Controls — a REAL array in the same binding forms must be untouched.
// ---------------------------------------------------------------------------
const realArr: number[] = [];
realArr.push(1);
realArr.push(2);
realArr[0] = 5;
realArr.length = 1;
console.log("real", realArr.length, realArr[0], Array.isArray(realArr));
const realLoop: number[] = [1, 2, 3, 4];
let realSum = 0;
for (let i = 0; i < realLoop.length; i++) {
  realSum += realLoop[i];
}
console.log("real bounded", realSum, realLoop.map((v) => v * 2).join(","));

// A plain object merely ANNOTATED as an array must degrade, never crash.
const lying = { length: 0 } as unknown as number[];
console.log("lying", lying.length, lying[0]);

console.log("isArray", Array.isArray(asConst), Array.isArray(realArr));
console.log("done");
