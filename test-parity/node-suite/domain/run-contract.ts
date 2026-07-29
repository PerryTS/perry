import domain from "node:domain";

const d = domain.create();
const result = d.run(
  function (this: any, a: string, b: string) {
    console.log(this === d, domain.active === d, a, b);
    return "result";
  },
  "one",
  "two",
);
console.log(result, String(domain.active));
