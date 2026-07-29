import domain from "node:domain";

const a = domain.create();
const b = domain.create();
const c = domain.create();
a.enter();
b.enter();
c.enter();
b.exit();
console.log(
  (domain as any)._stack.length,
  domain.active === a,
  (process as any).domain === a,
);
a.exit();
