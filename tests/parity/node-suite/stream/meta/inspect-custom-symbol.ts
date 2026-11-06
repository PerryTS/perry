import { Readable } from "node:stream";
import * as util from "node:util";
// util.inspect.custom symbol — Stream instances may override util.inspect's
// formatting via Symbol.for("nodejs.util.inspect.custom").
const r = new Readable({ read() {} });
const sym = util.inspect.custom;
console.log("custom inspect symbol is symbol:", typeof sym === "symbol");
console.log("inspect output is string:", typeof util.inspect(r) === "string");
