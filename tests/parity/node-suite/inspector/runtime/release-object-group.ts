import { Session } from "node:inspector";

const session = new Session();
session.connect();
const post = (method: string, params?: object) =>
  new Promise<any>((resolve, reject) =>
    session.post(
      method,
      params,
      (err, value) => err ? reject(err) : resolve(value),
    )
  );
try {
  const first = await post("Runtime.evaluate", {
    expression: "({ first: true })",
    objectGroup: "parity-group",
  });
  const second = await post("Runtime.evaluate", {
    expression: "({ second: true })",
    objectGroup: "parity-group",
  });
  console.log(
    "ids:",
    typeof first.result.objectId,
    typeof second.result.objectId,
    first.result.objectId !== second.result.objectId,
  );
  console.log(
    "release group:",
    Object.keys(
      await post("Runtime.releaseObjectGroup", { objectGroup: "parity-group" }),
    ).length,
  );
  for (const objectId of [first.result.objectId, second.result.objectId]) {
    await new Promise<void>((resolve) =>
      session.post("Runtime.getProperties", { objectId }, (cause) => {
        const error = cause as unknown as
          | { code?: string; message?: string }
          | null;
        console.log(
          "released:",
          error?.code ?? "<none>",
          error?.message?.includes("Could not find object with given id") ??
            false,
        );
        resolve();
      })
    );
  }
} finally {
  session.disconnect();
}
