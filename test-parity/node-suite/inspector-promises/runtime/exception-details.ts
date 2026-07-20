import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const pending = session.post("Runtime.evaluate", {
    expression: 'throw new TypeError("promise marker")',
  });
  const value = await pending;
  console.log(
    "resolved:",
    value.result.type,
    value.result.subtype,
    value.result.className,
    value.result.description?.startsWith("TypeError: promise marker"),
  );
  console.log(
    "exception:",
    value.exceptionDetails.text,
    typeof value.exceptionDetails.exceptionId,
    value.exceptionDetails.exception?.className,
    value.exceptionDetails.exception?.description?.startsWith(
      "TypeError: promise marker",
    ),
  );
} finally {
  session.disconnect();
}
