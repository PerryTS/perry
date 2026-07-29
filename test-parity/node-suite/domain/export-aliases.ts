import domain from "node:domain";

console.log(domain.create === domain.createDomain);
console.log(domain.create() instanceof domain.Domain);
console.log(domain.createDomain() instanceof domain.Domain);
