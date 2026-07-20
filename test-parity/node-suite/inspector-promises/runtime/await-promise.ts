import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const pending = session.post("Runtime.evaluate", {
    expression: "Promise.resolve(40).then((value) => value + 2)",
    awaitPromise: true,
    returnByValue: true,
  });
  console.log("returned:", pending instanceof Promise);
  const value = await pending;
  console.log(
    "fulfilled:",
    value.result.type,
    value.result.value,
    Object.hasOwn(value, "exceptionDetails"),
  );
} finally {
  session.disconnect();
}
