import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  for (const expression of ["NaN", "Infinity", "-Infinity", "-0", "123n"]) {
    await new Promise<void>((resolve, reject) =>
      session.post(
        "Runtime.evaluate",
        { expression },
        (err, { result } = {} as any) => {
          if (err) return reject(err);
          console.log(
            expression,
            result.type,
            result.unserializableValue ?? "-",
            result.description ?? "-",
            Object.hasOwn(result, "value"),
          );
          resolve();
        },
      )
    );
  }
} finally {
  session.disconnect();
}
