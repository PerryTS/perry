function first() { return 11; }
function second() { return 17; }
function born() { return { method: first, closure: () => 23 }; }
const a = born();
console.log(a.method(), a.closure());
first = second;
const b = born();
console.log(a.method(), b.method(), b.closure());
const key = "method";
(b as any)[key] = () => 31;
console.log(a.method(), b.method(), Object.keys(b).join(","));
let sum = 0;
for (let i = 0; i < 100; i++) {
  const c = born();
  const churn = { f: () => i, item: { i } };
  sum += c.method() + c.closure() + churn.f();
}
console.log(sum);
