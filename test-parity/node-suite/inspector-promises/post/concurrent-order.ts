import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const settled: string[] = [];
  const first = session.post("Runtime.evaluate", {
    expression: "2 + 2",
    returnByValue: true,
  });
  const delayed = session.post("Runtime.evaluate", {
    expression:
      "new Promise((resolve) => { globalThis.__perryResolve = resolve; })",
    awaitPromise: true,
    returnByValue: true,
  });
  const third = session.post("Runtime.evaluate", {
    expression: "4 + 4",
    returnByValue: true,
  });
  first.then(
    () => settled.push("first"),
    () => settled.push("first rejected"),
  );
  delayed.then(
    () => settled.push("delayed"),
    () => settled.push("delayed rejected"),
  );
  third.then(
    () => settled.push("third"),
    () => settled.push("third rejected"),
  );

  const independent = await Promise.all([first, third]);
  console.log(
    "before release:",
    independent.map((entry) => entry.result.value).join(","),
    settled.join(","),
  );
  await session.post("Runtime.evaluate", {
    expression:
      "globalThis.__perryResolve(6); delete globalThis.__perryResolve",
  });
  const ordered = await Promise.all([first, delayed, third]);
  console.log(
    "ordered:",
    ordered.map((entry) => entry.result.value).join(","),
    settled.join(","),
  );
} finally {
  try {
    await session.post("Runtime.evaluate", {
      expression: "delete globalThis.__perryResolve",
    });
  } catch {
    // Cleanup must not mask the contract result on divergent runtimes.
  }
  session.disconnect();
}
