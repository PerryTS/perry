import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve, reject) =>
    session.post("Runtime.evaluate", {
      expression: 'throw new TypeError("marker")',
    }, (err, value) => {
      if (err) return reject(err);
      const { result, exceptionDetails } = value;
      console.log(
        "result:",
        result.type,
        result.subtype,
        result.className,
        result.description?.startsWith("TypeError: marker"),
        typeof result.objectId,
      );
      console.log(
        "exception:",
        exceptionDetails.text,
        typeof exceptionDetails.exceptionId,
        exceptionDetails.exception?.className,
        exceptionDetails.exception?.description?.startsWith(
          "TypeError: marker",
        ),
      );
      resolve();
    })
  );
} finally {
  session.disconnect();
}
