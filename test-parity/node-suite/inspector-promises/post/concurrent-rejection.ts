import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const first = session.post("Runtime.evaluate", { expression: "10 + 1" });
  const invalid = session.post("Missing.concurrent");
  const third = session.post("Runtime.evaluate", { expression: "10 + 3" });
  try {
    await Promise.all([first, invalid, third]);
    console.log("unexpected aggregate resolution");
  } catch (error) {
    const cause = error as { name?: string; code?: string; message?: string };
    console.log(
      "aggregate:",
      cause.name,
      cause.code,
      cause.message?.includes("-32601"),
    );
  }
  const [one, three] = await Promise.all([first, third]);
  try {
    await invalid;
  } catch (error) {
    const repeated = error as { code?: string };
    console.log(
      "independent:",
      one.result.value,
      three.result.value,
      repeated.code,
    );
  }
} finally {
  session.disconnect();
}
