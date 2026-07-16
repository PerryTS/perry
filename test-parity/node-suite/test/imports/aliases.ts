import test, { describe, it, suite } from "node:test";

console.log("test.test identity:", test.test === test);
console.log("it identity:", it === test);
console.log("describe identity:", describe === suite);
console.log("method identities:", test.skip === test.skip, test.todo === test.todo);
