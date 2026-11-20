// Alternating birth identities, tracking widths, spill learning and mutation.
const left: any = { inherited: 3 }, right: any = { inherited: 5 };
const held: any[] = [];
for (let i = 0; i < 180; i++) {
  const proto = i % 2 === 0 ? left : right;
  const o: any = Object.create(proto);
  o.a = i; o.b = i + 1;
  if (i < 8 || i > 100) {
    o.c = i + 2; o.d = i + 3; o.e = i + 4;
    o.f = i + 5; o.g = i + 6; o.h = i + 7; o.i = i + 8;
  }
  if (i % 3 === 0) Object.defineProperty(o, "computed", {
    get() { return this.a + this.inherited; }, enumerable: true, configurable: true
  });
  held.push(o);
}
let sum = 0;
for (let i = 0; i < held.length; i++) {
  const o = held[i], proto = i % 2 === 0 ? left : right;
  if (Object.getPrototypeOf(o) !== proto) throw new Error("birth prototype");
  sum += o.a + o.b + o.inherited + (o.i || 0);
  if (i % 3 === 0) sum += o.computed;
}
left.inherited = 7;
Object.defineProperty(right, "later", { get() { return 11; }, configurable: true });
const fresh: any = Object.create(right);
console.log("retained", held.length, sum, held[0].computed, fresh.later);
const nullish: any = Object.create(null, { x: { value: 13, enumerable: true } });
console.log("null", Object.getPrototypeOf(nullish) === null, nullish.x, Object.keys(nullish).join("|"));
