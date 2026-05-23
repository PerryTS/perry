// process.release — at minimum `{ name: "node" }`. Feature-detection
// branches on `process.release.name === "node"`. Regression cover for
// #1348 (Perry was returning a number sentinel, so `.name` exploded).
const r = process.release;
console.log("typeof:", typeof r);
console.log("name:", r.name);
console.log("sourceUrl typeof:", typeof r.sourceUrl);
console.log("headersUrl typeof:", typeof r.headersUrl);
