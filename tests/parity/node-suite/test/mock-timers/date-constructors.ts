import { mock } from "node:test";

mock.timers.enable({ apis: ["Date"], now: 100 });
console.log("no args:", new Date().getTime());
console.log("numeric args:", new Date(0).getTime(), new Date(250).getTime());
console.log("string arg:", new Date("1970-01-01T00:00:00.000Z").getTime());
console.log("static methods:", Date.parse("1970-01-01T00:00:00.000Z"), Date.UTC(1970, 0, 1));
mock.timers.reset();
