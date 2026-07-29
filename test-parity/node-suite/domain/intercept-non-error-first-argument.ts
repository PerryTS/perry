import domain from "node:domain";

const d = domain.create();
const intercepted = d.intercept((value: string) => {
  console.log(value, domain.active === d);
  return "result";
});
console.log(intercepted("not an error" as any, "kept"));
