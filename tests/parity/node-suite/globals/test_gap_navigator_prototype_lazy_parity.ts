// Navigator's constructor and prototype as node has them, read in the order
// a program meets them: the constructor's own keys and descriptors first
// (perry births the family lazily: listing keys must not need it), then the
// prototype's keys and descriptors, then the instance's identities.
const G: any = globalThis as any;
const N: any = G.Navigator;
const out: string[] = [];
out.push("ctor keys: " + JSON.stringify(Reflect.ownKeys(N).map(String)));
out.push("has prototype: " + String(Object.prototype.hasOwnProperty.call(N, "prototype")) + "," + String("prototype" in N));
for (const k of ["length", "name"]) {
  const d = Object.getOwnPropertyDescriptor(N, k)!;
  out.push(k + ": " + JSON.stringify(d.value) + "," + [d.writable, d.enumerable, d.configurable].join(","));
}
const pd = Object.getOwnPropertyDescriptor(N, "prototype")!;
out.push("prototype desc: " + [typeof pd.value, pd.writable, pd.enumerable, pd.configurable].join(","));
const P = N.prototype;
out.push("same prototype: " + String(pd.value === P));
out.push("proto keys: " + JSON.stringify(Reflect.ownKeys(P).map(String)));
for (const k of Reflect.ownKeys(P)) {
  const d: any = Object.getOwnPropertyDescriptor(P, k)!;
  const kind = "value" in d
    ? "data " + String(d.value === N) + "," + String(d.writable)
    : "get " + d.get.name + "," + d.get.length + "," + typeof d.set;
  out.push(String(k) + ": " + kind + "," + String(d.enumerable) + "," + String(d.configurable));
}
out.push("proto of proto: " + String(Object.getPrototypeOf(P) === Object.prototype));
out.push("ctor keys after: " + JSON.stringify(Reflect.ownKeys(N).map(String)));
out.push("instanceof: " + String(G.navigator instanceof N));
out.push("getPrototypeOf: " + String(Object.getPrototypeOf(G.navigator) === P));
out.push("constructor: " + String(G.navigator.constructor === N));
console.log(out.join("\n"));
