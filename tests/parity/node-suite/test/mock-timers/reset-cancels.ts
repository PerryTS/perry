import { mock } from "node:test";

mock.timers.enable({ apis: ["setTimeout"], now: 0 });
let called = false;
setTimeout(() => {
  called = true;
}, 5);
mock.timers.reset();
console.log("reset cancelled:", called);
try {
  mock.timers.tick(5);
  console.log("tick after reset: NO_THROW");
} catch (error) {
  console.log("tick after reset:", (error as any).code ?? (error as Error).name);
}
