import * as util from "node:util";
console.log("format function:", typeof util.format === "function");
console.log("inspect function:", typeof util.inspect === "function");
console.log("promisify function:", typeof util.promisify === "function");
console.log("types object:", typeof util.types === "object");
