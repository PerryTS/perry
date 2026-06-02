// process.config — build-time config object. Node exposes
// `{ variables, target_defaults }` populated from node-gyp's GYP file.
// Perry compiles AOT with no gyp file to surface, so the sub-objects
// are empty but the shape (typeof) is preserved. Regression cover for
// #1379. Asserts shape only since exact contents are Node-version /
// build-tooling specific.
const c = process.config;
console.log("typeof:", typeof c);
console.log("variables typeof:", typeof c.variables);
console.log("target_defaults typeof:", typeof c.target_defaults);
