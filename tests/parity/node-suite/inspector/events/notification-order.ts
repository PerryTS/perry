import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  const order: string[] = [];
  const done = new Promise<void>((resolve) => {
    session.once("Runtime.executionContextCreated", (message) => {
      order.push("specific");
      console.log(
        "specific shape:",
        message.method,
        typeof message.params.context.id,
        typeof message.params.context.name,
      );
    });
    const generic = (message: any) => {
      if (message.method === "Runtime.executionContextCreated") {
        order.push("generic");
        session.off("inspectorNotification", generic);
        resolve();
      }
    };
    session.on("inspectorNotification", generic);
  });
  session.post("Runtime.enable", (err, value) => {
    order.push("callback");
    console.log("callback:", err === null, Object.keys(value).length);
  });
  await done;
  await new Promise((resolve) => setImmediate(resolve));
  console.log("order:", order.join(","));
} finally {
  session.disconnect();
}
