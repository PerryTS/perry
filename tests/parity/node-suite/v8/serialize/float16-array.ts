import { deserialize, serialize } from "node:v8";

const source = new Float16Array([1.5, -2, 0]);
const result: any = deserialize(serialize(source));
console.log("brand:", result instanceof Float16Array, result.constructor.name);
console.log("shape:", result.length, result.byteLength);
console.log("values:", [...result].join(","));
console.log("fresh:", result !== source, result.buffer !== source.buffer);
