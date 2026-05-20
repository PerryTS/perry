import { format } from "node:util";
console.log("str-num:", format("%s + %d = %d", "a", 1, 2));
console.log("float:", format("%f", 1.5));
console.log("json:", format("obj=%j", { x: 1 }));
console.log("object:", format("o=%o", { a: 1 }));
console.log("percent:", format("100%% done"));
