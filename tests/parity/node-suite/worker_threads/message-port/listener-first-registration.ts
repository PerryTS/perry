import { MessageChannel } from "node:worker_threads";

function run(
  label: string,
  first: "on" | "once",
  second: "on" | "once",
): Promise<void> {
  return new Promise((resolve) => {
    const { port1, port2 } = new MessageChannel();
    let calls = 0;
    const listener = () => calls += 1;

    port1[first]("message", listener);
    port1[second]("message", listener);

    const observer = (value: number) => {
      if (value !== 2) return;
      port1.off("message", observer);
      const remaining = typeof (port1 as any).listenerCount === "function"
        ? (port1 as any).listenerCount("message")
        : "unsupported";
      console.log(`${label}:`, calls, remaining);
      port1.close();
    };
    port1.on("message", observer);
    port1.on("close", () => {
      port2.close();
      resolve();
    });
    port2.postMessage(1);
    port2.postMessage(2);
  });
}

async function main() {
  await run("on+on", "on", "on");
  await run("on+once", "on", "once");
  await run("once+on", "once", "on");
  await run("once+once", "once", "once");
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
