// process.allowedNodeEnvironmentFlags is a Set with the usual Set methods.
const f = process.allowedNodeEnvironmentFlags;
console.log("has method:", typeof f.has === "function");
console.log("size is number:", typeof f.size === "number");
