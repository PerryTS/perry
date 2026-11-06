import { Readable, pipeline } from "node:stream";
// pipeline accepts a single stream + callback (rare but legal; mostly a smoke
// test against the API's flexibility).
const src = Readable.from(["a"]);
src.on("data", () => {});
pipeline(src, (err) => console.log("err:", err === null || err === undefined));
