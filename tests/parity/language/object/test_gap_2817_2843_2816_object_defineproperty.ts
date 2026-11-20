// Gap test for #2817 / #2843 / #2816:
//   #2817 — Object.defineProperty / defineProperties descriptor validation + TypeErrors
//   #2843 — Object.defineProperty invariants on frozen / sealed / non-extensible objects
//   #2816 — Object.create descriptor-form + prototype validation

function expectThrow(fn: () => void): string {
  try {
    fn();
  } catch (e: any) {
    return `${e.name}: ${e.message}`;
  }
  return "NO THROW";
}

function expectThrowName(fn: () => void): string {
  try {
    fn();
  } catch (e: any) {
    return e.name;
  }
  return "NO THROW";
}

// ---------- #2817: valid descriptors still work ----------
const o: any = {};
Object.defineProperty(o, "hidden", { value: 1, enumerable: false });
console.log("keys", JSON.stringify(Object.keys(o)));
console.log("hidden enumerable", Object.getOwnPropertyDescriptor(o, "hidden")!.enumerable);
console.log("hidden value", o.hidden);

const acc: any = {};
let backing = 5;
Object.defineProperty(acc, "v", {
  get() {
    return backing;
  },
  set(x: number) {
    backing = x;
  },
  enumerable: true,
});
console.log("acc get", acc.v);
acc.v = 9;
console.log("acc get2", acc.v);

// ---------- #2817: invalid inputs throw exact Node messages ----------
console.log(expectThrow(() => Object.defineProperty({}, "x")));
console.log(expectThrow(() => Object.defineProperty(1 as any, "x", { value: 1 })));
console.log(expectThrow(() => Object.defineProperty(null as any, "x", { value: 1 })));
console.log(expectThrow(() => Object.defineProperty({}, "x", 1 as any)));
console.log(
  expectThrow(() => Object.defineProperty({}, "x", { value: 1, get() { return 2; } } as any)),
);
console.log(expectThrow(() => Object.defineProperty({}, "x", { get: 1 } as any)));
console.log(expectThrow(() => Object.defineProperty({}, "x", { set: 1 } as any)));
console.log(expectThrow(() => Object.defineProperties({}, null as any)));
console.log(expectThrow(() => Object.defineProperties(1 as any, { a: { value: 1 } })));
console.log(expectThrow(() => Object.defineProperties({}, { x: 1 as any })));

// ---------- #2843: frozen / sealed / non-extensible invariants ----------
const frozen = Object.freeze({ a: 1 });
console.log(expectThrow(() => Object.defineProperty(frozen, "a", { value: 2 })));
console.log(expectThrow(() => Object.defineProperty(frozen, "b", { value: 2 })));

const sealed = Object.seal({ a: 1 });
// Allowed: rewrite existing writable data prop value.
Object.defineProperty(sealed, "a", { value: 2 });
console.log("sealed a", (sealed as any).a);
console.log(expectThrow(() => Object.defineProperty(sealed, "a", { configurable: true })));
console.log(expectThrowName(() => Object.defineProperty(sealed, "b", { value: 2 })));

const nonExt = Object.preventExtensions({ a: 1 });
// Allowed: rewrite existing prop.
Object.defineProperty(nonExt, "a", { value: 2 });
console.log("nonExt a", (nonExt as any).a);
console.log(expectThrowName(() => Object.defineProperty(nonExt, "b", { value: 2 })));

// ---------- #2816: Object.create descriptor-form + prototype validation ----------
const proto = { p: 1 };
const obj = Object.create(proto, {
  a: { value: 2, enumerable: true },
  b: { value: 3, enumerable: false },
});
console.log("create p", obj.p);
console.log("create a", obj.a);
console.log("create b", obj.b);
console.log("create keys", JSON.stringify(Object.keys(obj)));
console.log("create b enumerable", Object.getOwnPropertyDescriptor(obj, "b")!.enumerable);

console.log(expectThrowName(() => Object.create(1 as any)));
console.log(expectThrowName(() => Object.create("x" as any)));
console.log(expectThrowName(() => Object.create(undefined as any)));
console.log("create null proto", Object.getPrototypeOf(Object.create(null)));

const justProto = Object.create(proto);
console.log("justProto p", justProto.p);
console.log("justProto keys", JSON.stringify(Object.keys(justProto)));
