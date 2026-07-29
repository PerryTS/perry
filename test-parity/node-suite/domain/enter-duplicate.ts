import domain from "node:domain";

const d = domain.create();
d.enter();
d.enter();
console.log((domain as any)._stack.length, domain.active === d);
d.exit();
console.log((domain as any)._stack.length, String(domain.active));
