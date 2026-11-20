// await runs PromiseResolve(%Promise%, v): for a promise v it READS v.constructor
// (an own accessor included) and only reuses v when it is %Promise%.
const log: string[] = [];
const p1 = Promise.resolve(1);
let g1 = 0;
Object.defineProperty(p1, "constructor", { get() { g1++; return Promise; } });
const p2 = Promise.resolve(2);
let g2 = 0;
Object.defineProperty(p2, "constructor", { get() { g2++; return Object; } });
(async () => {
  const a = await p1;
  log.push("p1 " + a + " getter=" + g1);
  const b = await p2;
  log.push("p2 " + b + " getter=" + g2);
  console.log(log.join("\n"));
})();
