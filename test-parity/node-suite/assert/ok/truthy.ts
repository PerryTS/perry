import assert from "node:assert";

for (const value of [true, 1, "x", {}, []]) assert.ok(value);
console.log("truthy ok");
