import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const evaluated = await session.post("Runtime.evaluate", {
    expression: "({ alpha: 1, beta: 'two' })",
  });
  const objectId = evaluated.result.objectId;
  const properties = await session.post("Runtime.getProperties", {
    objectId,
    ownProperties: true,
  });
  const own = properties.result
    .filter((entry) => entry.enumerable)
    .map((entry) => `${entry.name}:${entry.value?.value}`)
    .sort();
  console.log("properties:", own.join(","));
  const released = await session.post("Runtime.releaseObject", { objectId });
  console.log("release:", Reflect.ownKeys(released).length);
  try {
    await session.post("Runtime.getProperties", { objectId });
    console.log("unexpected retained object");
  } catch (error) {
    const cause = error as { name?: string; code?: string; message?: string };
    console.log(
      "released error:",
      cause.name,
      cause.code,
      cause.message?.includes("-32000"),
      cause.message?.includes("Could not find object"),
    );
  }
} finally {
  session.disconnect();
}
