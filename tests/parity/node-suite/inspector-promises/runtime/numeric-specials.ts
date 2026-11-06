import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  for (const expression of ["NaN", "Infinity", "-0", "123n"]) {
    const { result } = await session.post("Runtime.evaluate", { expression });
    console.log(
      expression,
      result.type,
      result.unserializableValue ?? "<none>",
      result.description ?? "<none>",
      Object.hasOwn(result, "value"),
    );
  }
} finally {
  session.disconnect();
}
