import { format } from "node:util";
console.log("trailing:", format("hello", "x", 2));
console.log("missing:", format("%s %s", "x"));
console.log("non-string:", format({ a: 1 }, [2]));
