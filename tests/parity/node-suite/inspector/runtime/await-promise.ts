import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (
    const expression of [
      "Promise.resolve(42)",
      "(async () => 'ready')()",
    ] as const
  ) {
    await new Promise<void>((resolve, reject) =>
      session.post("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      }, (err, value) => {
        if (err) return reject(err);
        console.log(
          expression,
          value.result.type,
          JSON.stringify(value.result.value),
          value.exceptionDetails === undefined,
        );
        resolve();
      })
    );
  }
} finally {
  session.disconnect();
}
