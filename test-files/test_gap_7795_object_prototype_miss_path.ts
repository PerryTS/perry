// #7795: the ordinary-object property-MISS path now resolves the default
// `Object.prototype` from the memoized, GC-healed `object_prototype_addr()`
// cache instead of re-running `globalThis.Object` (which interns an `"Object"`
// key string) plus `closure_get_dynamic_prop("prototype")` on every miss.
//
// The miss path is what answers `Get(v, "then")` for the spec thenable check on
// every `await` of a plain object, so it must keep answering EXACTLY as before:
// builtin `Object.prototype` members still read as functions, absent keys still
// read `undefined`, and a USER-installed `Object.prototype` property must still
// be visible on plain objects (including making them genuinely thenable).

const o: any = { a: 1 };

// Builtin Object.prototype members must still resolve through the miss path.
console.log("toString", typeof o.toString);
console.log("hasOwnProperty", typeof o.hasOwnProperty);
console.log("valueOf", typeof o.valueOf);
console.log("isPrototypeOf", typeof o.isPrototypeOf);
console.log("propertyIsEnumerable", typeof o.propertyIsEnumerable);
console.log("toLocaleString", typeof o.toLocaleString);
console.log("constructor", typeof o.constructor);
console.log("call-toString", o.toString());
console.log("call-hasOwn", o.hasOwnProperty("a"));

// A genuinely absent key still reads undefined.
console.log("absent", o.then, o.nope, o.zzz);

// A user-installed Object.prototype property must be visible on plain objects.
(Object.prototype as any).marker = 42;
console.log("proto-marker", o.marker);
const fresh: any = {};
console.log("proto-marker-fresh", fresh.marker);
delete (Object.prototype as any).marker;
console.log("proto-marker-deleted", o.marker, fresh.marker);

// An accessor installed on Object.prototype must still run.
Object.defineProperty(Object.prototype, "acc", {
  get() {
    return 99;
  },
  configurable: true,
});
console.log("proto-accessor", ({} as any).acc);
delete (Object.prototype as any).acc;
console.log("proto-accessor-gone", ({} as any).acc);

// The await path this optimises: a plain object is NOT thenable...
async function plain(): Promise<{ v: number }> {
  return { v: 7 };
}
plain().then((r: { v: number }) => {
  console.log("await-plain", r.v);

  // ...but installing `then` on Object.prototype DOES make plain objects
  // thenable, which the miss path must still observe.
  (Object.prototype as any).then = function (res: (x: number) => void) {
    res(123);
  };
  Promise.resolve()
    .then(() => ({ plainObj: true }) as any)
    .then((v: any) => {
      console.log("proto-then-assimilated", v);
      delete (Object.prototype as any).then;
    });
});
