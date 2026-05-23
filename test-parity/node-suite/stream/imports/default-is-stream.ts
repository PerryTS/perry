// `import Stream from "node:stream"` should bind the local to the legacy
// Stream constructor — typeof "function", with the Readable/Writable/...
// classes hung off it as statics. Perry was resolving the default import
// to a namespace object (typeof "object"), so feature-detect like
// `typeof Stream === "function"` returned false. Regression cover for
// #1535. Asserts only the typeof — the class-statics surface is tracked
// separately under #1531.
import Stream from "node:stream";
console.log("typeof:", typeof Stream);
