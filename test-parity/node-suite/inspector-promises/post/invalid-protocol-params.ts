import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  try {
    await session.post("Runtime.evaluate", {});
    console.log("unexpected resolution");
  } catch (error) {
    const cause = error as { name?: string; code?: string; message?: string };
    console.log(
      "invalid params:",
      cause.name,
      cause.code,
      cause.message?.includes("-32602"),
      cause.message?.includes("Invalid parameters"),
    );
  }
} finally {
  session.disconnect();
}
