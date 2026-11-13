// A class-annotated method call `p.m()` on a value that is really an Array, a Date or a typed array: node finds no
// `m` on it and throws TypeError (`p.m is not a function`). Before, Perry's by-name dispatch answered an empty object.
// Forms: parameter, class field, local. Negative controls: a real instance, a subclass instance, an Array / Date
// whose prototype chain does carry `m` (an own property, Array.prototype.m), a plain object with its own `m`, null.
class P {
  x: number = 7;
  m(): string {
    return "m" + this.x;
  }
}
class PS extends P {
  m(): string {
    return "ps" + this.x;
  }
}
function mk(k: string): any {
  switch (k) {
    case "preal": return new P();
    case "psub": return new PS();
    case "arr": return [7, 8];
    case "date": return new Date(0);
    case "u8": return new Uint8Array(2);
    case "f64": return new Float64Array(2);
    case "i32": return new Int32Array(2);
    case "ownm": { const a: any = [1]; a.m = () => "own"; return a; }
    case "obj": return { m: () => "objm" };
    case "null": return null;
  }
  return undefined;
}
function viaParam(p: P): any { return p.m(); }
class Holder {
  p: P = new P();
  run(): any { return this.p.m(); }
}
function viaLocal(v: any): any { const p: P = v; return p.m(); }
function show(f: () => any): string {
  try {
    const r = f();
    return typeof r === "object" && r !== null ? "object" : String(r);
  } catch (e: any) {
    return "THROW " + e.name;
  }
}
for (const k of ["preal", "psub", "arr", "date", "u8", "f64", "i32", "ownm", "obj", "null"]) {
  const h = new Holder();
  h.p = mk(k);
  console.log(k, show(() => viaParam(mk(k))), show(() => h.run()), show(() => viaLocal(mk(k))));
}
(Array.prototype as any).m = function () { return "proto" + this.length; };
console.log("proto", show(() => viaParam(mk("arr"))), show(() => viaLocal(mk("arr"))));
