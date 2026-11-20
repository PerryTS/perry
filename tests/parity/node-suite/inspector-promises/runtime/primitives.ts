import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  for (const expression of ["undefined", "null", "true", '"text"', "42"]) {
    const { result } = await session.post("Runtime.evaluate", { expression });
    console.log(
      expression,
      result.type,
      result.subtype ?? "<none>",
      Object.hasOwn(result, "value") ? String(result.value) : "<none>",
      result.description ?? "<none>",
    );
  }
} finally {
  session.disconnect();
}
