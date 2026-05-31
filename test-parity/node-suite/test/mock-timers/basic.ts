import { mock } from "node:test";

function codeOf(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (err) {
    return (err as any).code ?? (err as Error).name;
  }
}

console.log("pre tick:", codeOf(() => mock.timers.tick(1)));

mock.timers.enable({
  apis: ["Date", "setTimeout", "setInterval"],
  now: 1000,
});

const events: string[] = [];
setTimeout((label: string) => events.push(`${label}:${Date.now()}`), 10, "timeout");

let interval: any;
interval = setInterval((label: string) => {
  events.push(`${label}:${Date.now()}`);
  if (events.filter((event) => event.startsWith("interval")).length === 2) {
    clearInterval(interval);
  }
}, 5, "interval");

console.log("start:", Date.now(), new Date().toISOString());
mock.timers.tick(4);
console.log("after4:", JSON.stringify(events));
mock.timers.tick(1);
console.log("after5:", JSON.stringify(events));
mock.timers.tick(5);
console.log("after10:", JSON.stringify(events));

setTimeout(() => events.push(`runAll:${Date.now()}`), 25);
mock.timers.runAll();
console.log("afterRunAll:", JSON.stringify(events));

mock.timers.setTime(2000);
console.log("afterSetTime:", Date.now());
mock.timers.reset();
console.log("resetReal:", Date.now() === 2000 ? "mocked" : "real");

console.log(
  "bad api:",
  codeOf(() => mock.timers.enable({ apis: ["nextTick"] } as any)),
);
console.log("bad setTime:", codeOf(() => mock.timers.setTime("x" as any)));
