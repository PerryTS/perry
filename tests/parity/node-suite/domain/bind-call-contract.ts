import domain from "node:domain";

const d = domain.create();
const receiver = { value: 7 };
const bound = d.bind(function (this: any, a: number, b: number) {
  console.log(this === receiver, domain.active === d, a, b);
  return this.value + a + b;
});
console.log(bound.call(receiver, 2, 3), String(domain.active));
