import domain from "node:domain";

const a = domain.create();
const b = domain.create();
a.enter();
b.exit();
console.log((domain as any)._stack.length, domain.active === a);
a.exit();
