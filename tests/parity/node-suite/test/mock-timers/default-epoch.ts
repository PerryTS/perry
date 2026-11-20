import { mock } from "node:test";

mock.timers.enable({ apis: ["Date"] });
console.log("default epoch:", Date.now() === 0, new Date().getTime() === 0);
mock.timers.reset();
