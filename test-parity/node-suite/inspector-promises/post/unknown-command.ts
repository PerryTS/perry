import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  try {
    await session.post("Perry.missing");
    console.log("unexpected resolution");
  } catch (error) {
    const cause = error as { name?: string; code?: string; message?: string };
    console.log(
      "unknown:",
      cause.name,
      cause.code,
      cause.message?.includes("-32601"),
      cause.message?.includes("'Perry.missing' wasn't found"),
    );
  }
} finally {
  session.disconnect();
}
