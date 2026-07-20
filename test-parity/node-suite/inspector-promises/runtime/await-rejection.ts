import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const value = await session.post("Runtime.evaluate", {
    expression: 'Promise.reject(new RangeError("async marker"))',
    awaitPromise: true,
  });
  console.log(
    "resolved rejection:",
    value.result.type,
    value.result.subtype,
    value.result.className,
    value.result.description?.startsWith("RangeError: async marker"),
    value.exceptionDetails.text,
    value.exceptionDetails.exception?.className,
  );
} finally {
  session.disconnect();
}
