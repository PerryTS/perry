// The prototype's family never becomes the newborn receiver's layout kind.
class Holder { field = 7; }
const fn: any = () => 11;
const protos: any[] = [null, { field: 3 }, new Holder(), [5, 7], fn];
const held: any[] = [];
for (let i = 0; i < 120; i++) {
  const proto = protos[i % protos.length];
  const o: any = Object.create(proto);
  o.own = i;
  Object.defineProperty(o, "computed", {
    get() { return this.own + 13; }, configurable: true, enumerable: true
  });
  held.push([proto, o]);
}
let total = 0;
for (const [proto, o] of held) {
  if (Object.getPrototypeOf(o) !== proto || Array.isArray(o) || typeof o !== "object")
    throw new Error("inherited birth kind");
  total += o.computed;
  Object.setPrototypeOf(o, null);
  delete o.computed;
  o.computed = 17;
  if (Object.getPrototypeOf(o) !== null) throw new Error("new prototype");
  total += o.computed;
}
console.log("prototype families", held.length, total, fn());
for (const bad of [undefined, 1, true, "x", Symbol("bad")]) {
  try { Object.create(bad); console.log("accepted"); }
  catch (e: any) { console.log("invalid", e instanceof TypeError); }
}
