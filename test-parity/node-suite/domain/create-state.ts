import domain from "node:domain";

const first = domain.create();
const second = domain.create();
console.log(first !== second);
console.log(Array.isArray(first.members), first.members.length);
console.log(String((first as any).domain), String(domain.active));
