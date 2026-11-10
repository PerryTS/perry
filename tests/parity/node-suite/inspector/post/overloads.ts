import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  const order: string[] = [];
  await new Promise<void>((resolve) => {
    order.push("before");
    const returned = session.post("Runtime.evaluate", (err, result) => {
      order.push("callback");
      const error = err as unknown as
        | { code?: string; message?: string }
        | null;
      console.log(
        "method callback:",
        error?.code ?? "<none>",
        error?.message?.includes("Invalid parameters") ?? false,
        result === undefined,
      );
      resolve();
    });
    order.push("after");
    console.log("return:", returned === undefined);
  });
  await new Promise<void>((resolve) => {
    session.post("Runtime.evaluate", null as never, (err, result) => {
      const error = err as unknown as
        | { code?: string; message?: string }
        | null;
      console.log(
        "null params:",
        error?.code ?? "<none>",
        error?.message?.includes("Invalid parameters") ?? false,
        result === undefined,
      );
      resolve();
    });
  });
  console.log("order:", order.join(","));
} finally {
  session.disconnect();
}
