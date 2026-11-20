import domain from "node:domain";

console.log(Object.getOwnPropertyNames(domain.Domain.prototype).join(","));
console.log(
  Object.prototype.hasOwnProperty.call(domain.Domain.prototype, "members"),
);
console.log((domain.Domain.prototype as any).members);
