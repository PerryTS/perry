import { mock } from "node:test";

mock.timers.enable({ apis: ["Date"], now: 1_000 });
console.log("initial:", Date.now(), new Date().toISOString());
mock.timers.setTime(2_500);
console.log("advanced:", Date.now(), new Date().toISOString());
mock.timers.setTime(500);
console.log("rewound:", Date.now(), new Date().toISOString());
mock.timers.reset();
