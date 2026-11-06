import { mock } from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}

console.log("runAll disabled:", codeOf(() => mock.timers.runAll()));
console.log("tick disabled:", codeOf(() => mock.timers.tick()));
console.log("setTime disabled:", codeOf(() => mock.timers.setTime(1)));
console.log("null options:", codeOf(() => mock.timers.enable(null as any)));
mock.timers.reset();
console.log("number options:", codeOf(() => mock.timers.enable(1 as any)));
mock.timers.reset();
console.log("bad api type:", codeOf(() => mock.timers.enable({ apis: [1 as any] })));
console.log("bad now:", codeOf(() => mock.timers.enable({ now: -1 })));
mock.timers.enable({ apis: ["Date"], now: 0 });
console.log("negative tick:", codeOf(() => mock.timers.tick(-1)));
console.log("infinite tick:", codeOf(() => mock.timers.tick(Infinity)));
mock.timers.reset();
