import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const omitted = await session.post("Runtime.enable");
  const nullValue = await session.post("Runtime.disable", null as never);
  const undefinedValue = await session.post("Runtime.enable", undefined);
  console.log(
    "empty results:",
    Reflect.ownKeys(omitted).length,
    Reflect.ownKeys(nullValue).length,
    Reflect.ownKeys(undefinedValue).length,
  );
} finally {
  session.disconnect();
}
