import { inspect } from "node:util";

console.log("cause:", inspect(new Error("outer", { cause: new Error("inner") })));
console.log("aggregate:", inspect(new AggregateError([new Error("a"), "b"], "agg")));
