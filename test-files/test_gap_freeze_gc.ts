const keep: any[] = [];
const proto = { retained: "prototype" };
const symbol = Symbol("retained");
let sum = 0;
for (let i = 0; i < 120; i++) {
  const o: any = Object.create(proto);
  o.id = i; o.kind = i % 16; o.name = "type";
  Object.defineProperty(o, "access", {
    get() { return this.id + 3; }, set(v: number) { sum += v; },
    enumerable: true, configurable: true
  });
  o[symbol] = { id: i + 1 };
  if (i % 3 === 0) Object.preventExtensions(o);
  if (i % 2 === 0) Object.seal(o);
  Object.freeze(o); keep.push(o);
}
for (let i = 0; i < keep.length; i++) {
  const o = keep[i];
  o.access = 1;
  sum += o.id + o.access + o[symbol].id;
  if (!Object.isFrozen(o) || Object.getPrototypeOf(o) !== proto || o.name !== "type") throw Error("retained integrity");
}
console.log("gc", keep.length, sum, keep[0].retained, Object.keys(keep[119]).join(","));
