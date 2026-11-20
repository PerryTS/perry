import assert from "node:assert";

const e = new assert.AssertionError({ message: "boom" });
console.log("name:", e.name);
console.log("code:", e.code);
console.log("message:", e.message);
console.log("instanceof Error:", e instanceof Error);
