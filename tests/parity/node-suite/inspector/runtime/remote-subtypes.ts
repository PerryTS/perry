import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (
    const expression of [
      "[]",
      "/marker/gi",
      "new Date(0)",
      "new Map([[1, 2]])",
      "new Set([1])",
      "(function named() {})",
      "new Error('marker')",
    ]
  ) {
    await new Promise<void>((resolve, reject) =>
      session.post(
        "Runtime.evaluate",
        { expression },
        (err, { result } = {} as any) => {
          if (err) return reject(err);
          console.log(
            expression,
            result.type,
            result.subtype ?? "-",
            result.className ?? "-",
            typeof result.objectId,
          );
          resolve();
        },
      )
    );
  }
} finally {
  session.disconnect();
}
