import { MessageChannel } from "node:worker_threads";

function closeBeforeDispatch(): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    const seen: number[] = [];
    port1.on("message", (value) => seen.push(value));
    port1.on("close", () => {
      console.log("outside dispatch:", seen.join(",") || "empty");
      port2.close();
      resolve();
    });
    port2.postMessage(1);
    port2.postMessage(2);
    port1.close();
  });
}

function closeInsideDispatch(): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    const seen: number[] = [];
    port1.on("message", (value) => {
      seen.push(value);
      if (value === 1) {
        port1.close();
      }
    });
    port1.on("close", () => {
      console.log("inside dispatch:", seen.join(",") || "empty");
      port2.close();
      resolve();
    });
    port2.postMessage(1);
    port2.postMessage(2);
    port2.postMessage(3);
  });
}

async function main() {
  await closeBeforeDispatch();
  await closeInsideDispatch();
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
