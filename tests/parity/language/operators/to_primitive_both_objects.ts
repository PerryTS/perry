// OrdinaryToPrimitive step 6: when toString and valueOf both yield objects,
// every conversion throws TypeError, including when the object is a native
// stream/handle and when the conversion is a property-key one.
const objs: any[] = [{ tag: 1 }, (ReadableStream as any).from([1]), new Map(), [], new Error("x")];
for (let i = 0; i < objs.length; i++) {
  const u = objs[i];
  class S { toString(): any { return u; } }
  const s: any = new S();
  const o: any = { a: 1 };
  const fs: any[] = [() => { o[s] = 1; }, () => o[s], () => s in o, () => String(s), () => s + 1, () => `${s}`, () => +s];
  for (let j = 0; j < fs.length; j++) {
    try { fs[j](); console.log(i, j, "no throw"); } catch (e: any) { console.log(i, j, e instanceof TypeError, e.message); }
  }
}
const ok: any = { toString() { return {}; }, valueOf() { return 7; } };
console.log(ok + 1, String(ok));
