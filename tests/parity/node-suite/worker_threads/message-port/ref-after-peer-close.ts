import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

port1.on("message", () => {});
port1.on("close", () => {
  port1.ref();
  port1.onmessage = () => {};
  console.log(
    "after peer close:",
    port1.hasRef(),
    typeof (port1 as any).listenerCount === "function"
      ? (port1 as any).listenerCount("message")
      : "unsupported",
    typeof port1.onmessage,
  );
  port1.close();
});
port2.close();
