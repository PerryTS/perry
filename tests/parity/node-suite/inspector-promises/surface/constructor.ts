import { Session } from "node:inspector/promises";

try {
  Session();
  console.log("unexpected call");
} catch (error) {
  const cause = error as { name?: string; message?: string };
  console.log(
    "without new:",
    cause.name,
    cause.message?.includes("cannot be invoked without 'new'"),
  );
}
const session = new Session("ignored" as never);
try {
  console.log("extra argument:", session instanceof Session);
  session.connect();
  const value = await session.post("Runtime.evaluate", { expression: "1" });
  console.log("usable:", value.result.value);
} finally {
  session.disconnect();
}
