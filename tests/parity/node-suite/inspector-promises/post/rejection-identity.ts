import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const pending = session.post("Missing.identity");
  let first: unknown;
  let repeated: unknown;
  try {
    await pending;
  } catch (error) {
    first = error;
  }
  try {
    await pending;
  } catch (error) {
    repeated = error;
  }
  let separate: unknown;
  try {
    await session.post("Missing.identity");
  } catch (error) {
    separate = error;
  }
  const cause = first as { name?: string; code?: string; message?: string };
  console.log(
    "identity:",
    first === repeated,
    first !== separate,
    cause.name,
    cause.code,
    cause.message?.includes("-32601"),
  );
} finally {
  session.disconnect();
}
