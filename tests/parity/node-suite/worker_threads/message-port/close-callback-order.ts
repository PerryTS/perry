import { MessageChannel } from "node:worker_threads";

function first(): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    const order: string[] = [];
    port1.on("close", () => order.push("A"));
    port1.close(() => order.push("B"));
    port1.on("close", () => {
      order.push("C");
      console.log("first:", order.join(","));
      port2.close();
      resolve();
    });
    order.push("sync");
  });
}

function second(): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    const order: string[] = [];
    port1.close(() => order.push("B"));
    port1.on("close", () => {
      order.push("C");
      console.log("second:", order.join(","));
      port2.close();
      resolve();
    });
  });
}

async function main() {
  await first();
  await second();
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
