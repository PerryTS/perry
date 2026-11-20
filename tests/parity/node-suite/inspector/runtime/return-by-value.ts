import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (
    const expression of [
      "({ answer: 42, nested: { ok: true } })",
      '[1, "two", null]',
    ]
  ) {
    await new Promise<void>((resolve, reject) =>
      session.post(
        "Runtime.evaluate",
        { expression, returnByValue: true },
        (err, { result } = {} as any) => {
          if (err) return reject(err);
          console.log(
            result.type,
            result.subtype ?? "-",
            JSON.stringify(result.value),
            Object.hasOwn(result, "objectId"),
          );
          resolve();
        },
      )
    );
  }
} finally {
  session.disconnect();
}
