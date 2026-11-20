import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const object = await session.post("Runtime.evaluate", {
    expression: "({ alpha: 1, nested: { beta: true } })",
    returnByValue: true,
  });
  const array = await session.post("Runtime.evaluate", {
    expression: "[1, 'two', null]",
    returnByValue: true,
  });
  console.log(
    "object:",
    object.result.type,
    object.result.value.alpha,
    object.result.value.nested.beta,
    Object.hasOwn(object.result, "objectId"),
  );
  console.log(
    "array:",
    array.result.type,
    array.result.subtype,
    Array.isArray(array.result.value),
    array.result.value.join("|"),
  );
} finally {
  session.disconnect();
}
