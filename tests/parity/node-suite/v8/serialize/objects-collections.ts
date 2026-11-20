import { deserialize, serialize } from "node:v8";

const source = {
  object: { a: 1, nested: { b: "two" } },
  array: [1, , 3],
};
const result: any = deserialize(serialize(source));

console.log("object:", JSON.stringify(result.object));
console.log(
  "array:",
  result.array.length,
  1 in result.array,
  JSON.stringify(result.array),
);
console.log(
  "fresh:",
  result !== source,
  result.object !== source.object,
  result.array !== source.array,
);
