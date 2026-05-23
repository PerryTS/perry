// process.ref() / process.unref() — no-ops in Node, but they MUST exist as
// functions returning undefined. Regression cover for #1410 (Perry was
// reading them as numbers, so `typeof` lied and any invocation threw
// "value is not a function").
console.log("ref typeof:", typeof process.ref);
console.log("unref typeof:", typeof process.unref);
console.log("ref result:", process.ref());
console.log("unref result:", process.unref());
console.log("done");
