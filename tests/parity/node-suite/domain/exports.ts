import domain from "node:domain";

console.log(Object.keys(domain).join(","));
console.log(
  typeof domain.Domain,
  typeof domain.createDomain,
  typeof domain.create,
);
console.log(Array.isArray((domain as any)._stack), String(domain.active));
