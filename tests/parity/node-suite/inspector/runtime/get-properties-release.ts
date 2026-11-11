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
  const evaluated = await post("Runtime.evaluate", {
    expression: "({ alpha: 1, beta: true })",
    objectGroup: "fixture",
  });
  const objectId = evaluated.result.objectId;
  const properties = await post("Runtime.getProperties", {
    objectId,
    ownProperties: true,
  });
  const selected = properties.result.filter((item: any) =>
    item.name === "alpha" || item.name === "beta"
  );
  console.log(
    selected.map((item: any) =>
      `${item.name}:${item.enumerable}:${item.configurable}:${item.value.type}:${item.value.value}`
    ).join(","),
  );
  console.log(
    "internal arrays:",
    Array.isArray(properties.internalProperties),
    Array.isArray(properties.privateProperties),
  );
  console.log(
    "release:",
    Object.keys(await post("Runtime.releaseObject", { objectId })).length,
  );
  await new Promise<void>((resolve) =>
    session.post("Runtime.getProperties", { objectId }, (cause) => {
      const error = cause as unknown as
        | { code?: string; message?: string }
        | null;
      console.log(
        "released error:",
        error?.code ?? "<none>",
        error?.message?.includes("Could not find object with given id") ??
          false,
      );
      resolve();
    })
  );
} finally {
  session.disconnect();
}
