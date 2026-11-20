// process.env enumerates its keys; PATH is among them.
const keys = Object.keys(process.env);
console.log("has keys:", keys.length > 0);
console.log("all string values:", keys.every((k) => typeof process.env[k] === "string"));
console.log("includes PATH:", keys.includes("PATH"));
