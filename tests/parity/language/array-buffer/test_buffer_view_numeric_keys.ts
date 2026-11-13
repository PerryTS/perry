// An ArrayBuffer, a SharedArrayBuffer and a DataView have no integer-indexed elements: every numeric key is an
// ordinary property key, its ToString (`-1`, `1.5`, `4294967295`), and a key the view does not own is read from its
// prototype chain. Before, Perry dropped the store of a non-canonical numeric key and answered `undefined` for an
// index only the prototype holds.
// Negative controls: a canonical index store / read, `Object.keys`, `hasOwnProperty`, a typed array (whose `-1` is no
// property at all), a plain object.
function mk(k: string): any {
  switch (k) {
    case "ab": return new ArrayBuffer(4);
    case "sab": return new SharedArrayBuffer(4);
    case "dv": return new DataView(new ArrayBuffer(4));
    case "u8": return new Uint8Array(4);
    case "obj": return {};
  }
  return null;
}
for (const k of ["ab", "sab", "dv", "u8", "obj"]) {
  const out: string[] = [];
  let v = mk(k);
  v[-1] = 5;
  out.push(String(v[-1]), String(Object.keys(v)), String(v.hasOwnProperty("-1")));
  v = mk(k);
  v[1.5] = 6;
  out.push(String(v[1.5]), String(v["1.5"]), String(Object.keys(v)));
  v = mk(k);
  v[0] = 7;
  out.push(String(v[0]), String(Object.keys(v)), String(v.hasOwnProperty(0)));
  v = mk(k);
  v[4294967295] = 8;
  out.push(String(v[4294967295]), String(Object.keys(v)));
  v = mk(k);
  const p: any = Object.create(Object.getPrototypeOf(v));
  p[1] = "Q1";
  p[-2] = "Qm";
  Object.setPrototypeOf(v, p);
  out.push(String(v[1]), String(v[-2]), String(v[3]));
  console.log(k, out.join(" "));
}
