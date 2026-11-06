import domain from "node:domain";

const d = domain.create();
const receiver = { value: 7 };
const intercepted = d.intercept(function (this: any, a: number, b: number) {
  console.log(this === receiver, domain.active === d, a, b);
  return this.value + a + b;
});
console.log(intercepted.call(receiver, null, 2, 3), String(domain.active));
