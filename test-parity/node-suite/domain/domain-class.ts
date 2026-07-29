import domain from "node:domain";

const d = new domain.Domain();
console.log(domain.Domain.name, domain.Domain.length);
console.log(d instanceof domain.Domain, d.constructor === domain.Domain);
console.log(Object.getPrototypeOf(d) === domain.Domain.prototype);
