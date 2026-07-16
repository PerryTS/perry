import { mock } from "node:test";

mock.timers.enable({ apis: ["setTimeout"], now: 0 });
const events: string[] = [];
setTimeout(() => events.push("one"), 1);
setTimeout(() => events.push("two"), 2);
function tick(): string {
  try {
    mock.timers.tick();
    return "OK";
  } catch (error) {
    return (error as any).code ?? (error as Error).name;
  }
}
console.log("default tick:", tick(), JSON.stringify(events));
console.log("second tick:", tick(), JSON.stringify(events));
mock.timers.reset();
