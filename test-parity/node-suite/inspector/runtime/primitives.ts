import { Session } from "node:inspector";

const session = new Session();
session.connect();
function evaluate(expression: string): Promise<any> {
  return new Promise((resolve, reject) =>
    session.post(
      "Runtime.evaluate",
      { expression, returnByValue: true },
      (err, value) => err ? reject(err) : resolve(value),
    )
  );
}
try {
  for (
    const expression of ["undefined", "null", "true", "false", "42", '"hello"']
  ) {
    const { result } = await evaluate(expression);
    console.log(
      expression,
      result.type,
      result.subtype ?? "-",
      "value" in result ? JSON.stringify(result.value) : "<absent>",
      result.description ?? "-",
    );
  }
} finally {
  session.disconnect();
}
