import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
const order: string[] = [];
try {
  const pending = session.post("Runtime.evaluate", {
    expression: "new Promise(() => {})",
    awaitPromise: true,
  });
  order.push("posted");
  session.disconnect();
  order.push("disconnected");
  try {
    await pending;
    console.log("unexpected resolution");
  } catch (error) {
    order.push("rejected");
    const cause = error as { name?: string; code?: string; message?: string };
    console.log(
      "interrupted:",
      cause.name,
      cause.code,
      cause.message?.includes("-32000"),
      cause.message?.includes("Execution context was destroyed"),
    );
  }
  console.log("order:", order.join(","));
} finally {
  session.disconnect();
}
