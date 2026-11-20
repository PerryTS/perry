import { Session } from "node:inspector";

const session = new Session();
session.connect();
const order: string[] = [];
try {
  const completed = new Promise<void>((resolve) => {
    session.post("Runtime.evaluate", {
      expression: "new Promise(() => {})",
      awaitPromise: true,
    }, (cause, result) => {
      order.push("callback");
      const error = cause as unknown as {
        name?: string;
        code?: string;
        message?: string;
      } | null;
      console.log(
        error?.name ?? "<none>",
        error?.code ?? "<none>",
        error?.message ?? "<none>",
        result === undefined,
      );
      resolve();
    });
  });
  order.push("posted");
  session.disconnect();
  order.push("disconnected");
  await completed;
  console.log("order:", order.join(","));
} finally {
  session.disconnect();
}
