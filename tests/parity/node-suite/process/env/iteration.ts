// process.env supports `for...in` enumeration and Object.entries.
let count = 0;
for (const _ in process.env) count++;
console.log("for-in:", count > 0);
console.log("Object.entries:", Object.entries(process.env).length > 0);
console.log("Object.values:", Object.values(process.env).length > 0);
