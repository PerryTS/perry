import { mock } from "node:test";

mock.timers.enable({ apis: ["Date", "setTimeout"], now: 0 });
const events: string[] = [];
const timeout = setTimeout(() => events.push("timeout"), 10);
mock.timers.setTime(20);
console.log("after setTime:", Date.now(), JSON.stringify(events));
mock.timers.tick(1);
console.log("after tick:", Date.now(), JSON.stringify(events));
clearTimeout(timeout);
mock.timers.reset();
