import domain from "node:domain";

console.log(domain.create.name, domain.createDomain.name);
console.log(domain.create.length, domain.createDomain.length);
const createName = Object.getOwnPropertyDescriptor(domain.create, "name")!;
const createDomainName = Object.getOwnPropertyDescriptor(domain.createDomain, "name")!;
console.log(createName.writable, createName.enumerable, createName.configurable);
console.log(createDomainName.writable, createDomainName.enumerable, createDomainName.configurable);
