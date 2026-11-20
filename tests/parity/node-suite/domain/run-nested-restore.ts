import domain from "node:domain";

const outer = domain.create();
const inner = domain.create();
outer.run(() => {
  console.log("outer", domain.active === outer);
  inner.run(() => console.log("inner", domain.active === inner));
  console.log("restored", domain.active === outer);
});
console.log("done", String(domain.active), (domain as any)._stack.length);
