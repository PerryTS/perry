import { inspect } from "node:util";

console.log("map:", inspect(new Map([[{ a: 1 }, new Set([1, 2])]]), { depth: 3 }));
console.log("set:", inspect(new Set(["a", "b"])));
console.log("date:", inspect(new Date("2020-01-02T03:04:05.000Z")));
console.log("regexp:", inspect(/a+/gi));
