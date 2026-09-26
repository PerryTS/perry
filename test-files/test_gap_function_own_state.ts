// Function objects keep their own properties, deleted intrinsics and a
// recorded [[Prototype]] in the function object itself. Each case below
// differed from node before the every-receiver-shape closures stage.

// 1. bind() reads `length` with Get(): an accessor installed on the target runs.
function withAccessor(a: number, b: number) {
  return a + b;
}
Object.defineProperty(withAccessor, "length", { get: () => 42 });
console.log("accessor length", withAccessor.length, withAccessor.bind(null).length, withAccessor.bind(null, 1).length);

// 2. A deleted own `name` / `length` is inherited from Function.prototype.
function withDeletes(a: number, b: number) {
  return a + b;
}
delete (withDeletes as any).name;
delete (withDeletes as any).length;
console.log(
  "deleted",
  JSON.stringify((withDeletes as any).name),
  (withDeletes as any).length,
  Object.prototype.hasOwnProperty.call(withDeletes, "name"),
  withDeletes.call(null, 1, 2),
);

// 3. A recorded [[Prototype]]'s call/apply/bind win over Function.prototype's.
function withProto() {
  return 3;
}
const proto = {
  call(this: any, x: number) {
    return "proto call " + (this === withProto) + " " + x;
  },
  apply() {
    return "proto apply";
  },
  bind() {
    return "proto bind";
  },
};
Object.setPrototypeOf(withProto, proto);
console.log("recorded proto", (withProto as any).call(null, 7), (withProto as any).apply(), (withProto as any).bind());

// A recorded prototype that is itself a function: its Function.prototype
// methods still apply to the receiver.
function base(this: any, a: number) {
  return "base " + a;
}
function derived(this: any, a: number) {
  return "derived " + a;
}
Object.setPrototypeOf(derived, base);
console.log("function proto", derived.call(null, 1), derived.apply(null, [2]), derived.bind(null, 3)());

// Own properties on functions: values, re-adds after delete, many keys, order.
function bag() {}
(bag as any).b = 2;
(bag as any).a = 1;
(bag as any)[10] = "ten";
(bag as any)[2] = "two";
delete (bag as any).b;
(bag as any).b = 22;
console.log("own keys", Object.keys(bag).join(","), (bag as any).a, (bag as any).b, (bag as any)[2]);
for (let i = 0; i < 40; i++) (bag as any)["k" + i] = i;
let s = 0;
for (let i = 0; i < 40; i++) s += (bag as any)["k" + i];
console.log("many keys", s, Object.keys(bag).length);
