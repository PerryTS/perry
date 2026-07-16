import data from "../require/fixtures/data.json" with { type: "json" };
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const required = req("../require/fixtures/data.json");
const cache = req.cache[req.resolve("../require/fixtures/data.json")];
console.log("identity:", data === required, cache!.exports === data);
console.log("values:", required.name, required.count, required.nested.ok);
