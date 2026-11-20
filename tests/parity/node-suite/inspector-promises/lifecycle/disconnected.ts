import { Session } from "node:inspector/promises";

const session = new Session();
for (const label of ["before", "after"] as const) {
  if (label === "after") {
    session.connect();
    session.disconnect();
  }
  let pending: Promise<unknown> | undefined;
  let synchronous = false;
  try {
    try {
      pending = session.post("Runtime.enable");
    } catch (inner) {
      // synchronous throw: record it and re-throw so the real error reaches the
      // outer catch instead of being swallowed with pending undefined.
      synchronous = true;
      throw inner;
    }
    await pending;
    console.log(label, "unexpected");
  } catch (error) {
    const cause = error as { name?: string; code?: string };
    console.log(
      label,
      synchronous,
      pending instanceof Promise,
      cause.name,
      cause.code,
    );
  }
}
