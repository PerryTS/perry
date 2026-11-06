import { deserialize, serialize } from "node:v8";

const source = Object.assign(
  new TypeError("boom", { cause: new RangeError("root") }),
  {
    code: "E_SAMPLE",
  },
);
const result: any = deserialize(serialize(source));

console.log(
  "error:",
  result instanceof Error,
  result instanceof TypeError,
  result.name,
  result.message,
);
console.log(
  "cause:",
  result.cause instanceof RangeError,
  result.cause.name,
  result.cause.message,
);
console.log("stack type:", typeof result.stack);
console.log(
  "extra omitted:",
  Object.prototype.hasOwnProperty.call(result, "code"),
  result.code,
);
