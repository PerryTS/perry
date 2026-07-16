process.emitWarning = () => false;
import { stripTypeScriptTypes } from "node:module";

const source = [
  "type Pair<T> = [T, T];",
  "interface Box<T> { value: T }",
  "const pair: Pair<number> = [1, 2];",
  "const box = { value: pair[0] } satisfies Box<number>;",
  "const asserted = box.value as number;",
].join("\n");
const output = stripTypeScriptTypes(source);
console.log("line count:", output.split("\n").length);
console.log("length preserved:", output.length === source.length);
console.log(
  "runtime preserved:",
  output.includes("const pair"),
  output.includes("const box"),
  output.includes("const asserted"),
);
console.log(
  "types absent:",
  !output.includes("interface Box"),
  !output.includes("satisfies"),
  !output.includes("as number"),
);
console.log("prefix:", JSON.stringify(output.split("\n").slice(0, 3)));
