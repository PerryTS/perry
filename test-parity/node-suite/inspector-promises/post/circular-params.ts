import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const params: { self?: unknown } = {};
  params.self = params;
  let pending: Promise<unknown> | undefined;
  let synchronous = false;
  try {
    try {
      pending = session.post("Runtime.enable", params);
    } catch (inner) {
      // synchronous throw: record it and re-throw so the real error reaches the
      // outer catch instead of being swallowed with pending undefined.
      synchronous = true;
      throw inner;
    }
    await pending;
    console.log("unexpected resolution");
  } catch (error) {
    const cause = error as { name?: string; message?: string };
    console.log(
      "circular:",
      synchronous,
      pending instanceof Promise,
      cause.name,
      cause.message?.includes("circular structure"),
    );
  }
} finally {
  session.disconnect();
}
