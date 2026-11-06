import { MessageChannel } from "node:worker_threads";

function run(label: string, closeFirst: boolean): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    const events: string[] = [];

    port2.postMessage("first");
    port2.postMessage("second");
    port2.close();

    const onMessage = (value: unknown) => events.push(`message:${value}`);
    const onClose = () => {
      events.push("close");
      console.log(`${label}:`, events.join(","));
      port1.close();
      resolve();
    };

    if (closeFirst) {
      port1.on("close", onClose);
      port1.on("message", onMessage);
    } else {
      port1.on("message", onMessage);
      port1.on("close", onClose);
    }
  });
}

async function main() {
  await run("message-first", false);
  await run("close-first", true);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
