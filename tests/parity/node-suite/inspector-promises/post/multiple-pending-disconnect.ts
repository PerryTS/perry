import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const first = session.post("Runtime.evaluate", {
    expression: "new Promise(() => {})",
    awaitPromise: true,
  });
  const firstResult = first.then(
    () => "resolved",
    (error: { code?: string }) => error.code ?? "missing-code",
  );
  const second = session.post("Runtime.evaluate", {
    expression: "new Promise(() => {})",
    awaitPromise: true,
  });
  const secondResult = second.then(
    () => "resolved",
    (error: { code?: string }) => error.code ?? "missing-code",
  );

  session.disconnect();
  console.log("disconnect:", await firstResult, await secondResult);
} finally {
  session.disconnect();
}
