import { Readable } from "node:stream";
// unshift() returns undefined (it's not chainable like on()).
const r = new Readable({ read() {} });
r.push("a");
const result = r.unshift("b");
console.log("unshift returned:", result);
console.log("is undefined:", result === undefined);
