import { Session } from "node:inspector/promises";

const session = new Session();
try {
  session.connect();
  const first = await session.post("Runtime.evaluate", { expression: "6 * 7" });
  session.disconnect();
  session.connect();
  const second = await session.post("Runtime.evaluate", {
    expression: "7 * 8",
  });
  console.log("values:", first.result.value, second.result.value);
} finally {
  session.disconnect();
}
