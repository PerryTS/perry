import { Session } from "node:inspector/promises";

const session = new Session();
const order: string[] = [];
const specific = () => order.push("specific");
const generic = (message: { method?: string }) => {
  if (message.method === "Runtime.executionContextCreated") {
    order.push("generic");
  }
};
session.on("Runtime.executionContextCreated", specific);
session.on("inspectorNotification", generic);
session.connect();
try {
  const pending = session.post("Runtime.enable");
  const observed = pending.then((value) => {
    order.push("promise");
    return value;
  });
  await observed;
  console.log("order:", order.join(","));
  console.log(
    "listeners:",
    session.listenerCount("Runtime.executionContextCreated"),
    session.listenerCount("inspectorNotification"),
  );
} finally {
  session.off("Runtime.executionContextCreated", specific);
  session.off("inspectorNotification", generic);
  session.disconnect();
}
