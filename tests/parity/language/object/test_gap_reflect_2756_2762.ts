// Reflect return-value semantics: #2756 (set), #2757 (getPrototypeOf),
// #2758 (defineProperty), #2760 (deleteProperty), #2762 (extensibility).
const out: any = {};

// #2757 getPrototypeOf — actual prototype, not the target itself.
out.protoObj = Reflect.getPrototypeOf({}) === Object.prototype;
out.protoNull = Reflect.getPrototypeOf(Object.create(null));

// #2758 defineProperty — boolean success/failure.
const obj1: any = {};
out.def1 = Reflect.defineProperty(obj1, "x", { value: 1 });
const nonExt: any = {};
Object.preventExtensions(nonExt);
out.defNonExt = Reflect.defineProperty(nonExt, "x", { value: 1 });
const tgt: any = {};
Object.defineProperty(tgt, "x", { value: 1, configurable: false });
out.defNonConfig = Reflect.defineProperty(tgt, "x", { value: 2 });

// #2760 deleteProperty — boolean result.
const obj2: any = { x: 1 };
out.del1 = Reflect.deleteProperty(obj2, "x");
const fixed: any = {};
Object.defineProperty(fixed, "x", { value: 1, configurable: false });
out.delNonConfig = Reflect.deleteProperty(fixed, "x");
out.delNonConfigRemains = fixed.x;

// #2762 preventExtensions / isExtensible — boolean results.
const e1: any = {};
out.isExtBefore = Reflect.isExtensible(e1);
out.prevRet = Reflect.preventExtensions(e1);
out.isExtAfter = Reflect.isExtensible(e1);

// #2756 set — boolean result.
const frozen: any = {};
Object.defineProperty(frozen, "x", { value: 1, writable: false, configurable: true });
out.setNonWritable = Reflect.set(frozen, "x", 2);
const nonExt2: any = {};
Object.preventExtensions(nonExt2);
out.setNonExt = Reflect.set(nonExt2, "x", 1);
const okSet: any = {};
out.setOk = Reflect.set(okSet, "x", 3);
out.setOkVal = okSet.x;

console.log(JSON.stringify(out));

// #2762 — non-object targets throw TypeError.
for (const v of [1, null, undefined]) {
  try {
    Reflect.preventExtensions(v as any);
    console.log("prevNoThrow");
  } catch (e: any) {
    console.log("prevThrow", e.name);
  }
  try {
    Reflect.isExtensible(v as any);
    console.log("isExtNoThrow");
  } catch (e: any) {
    console.log("isExtThrow", e.name);
  }
}
