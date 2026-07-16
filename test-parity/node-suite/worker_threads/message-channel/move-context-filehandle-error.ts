import { open } from "node:fs/promises";
import { MessageChannel, moveMessagePortToContext } from "node:worker_threads";
import { createContext } from "node:vm";

const fixture =
  "test-parity/node-suite/worker_threads/worker-lifecycle/process-env-option-worker.cjs";

async function main() {
  const handle = await open(fixture, "r");
  const { port1, port2 } = new MessageChannel();
  let moved: any;

  try {
    moved = moveMessagePortToContext(port2, createContext({}));
  } catch (error: any) {
    console.log("setup:", error?.name, error?.code ?? "");
    port1.close();
    port2.close();
    await handle.close().catch(() => {});
    return;
  }

  const events: string[] = [];
  moved.onmessageerror = (event: any) => {
    events.push(`error:${event?.data?.code ?? "missing"}`);
  };
  moved.onmessage = async (event: any) => {
    if (event.data === "barrier") {
      console.log("events:", events.join(","));
      moved.close();
      port1.close();
      await handle.close().catch(() => {});
      return;
    }
    events.push(
      `message:${event.data?.constructor?.name ?? typeof event.data}`,
    );
  };
  moved.start();

  try {
    port1.postMessage(handle, [handle as any]);
    console.log("transfer: ok", handle.fd === -1);
  } catch (error: any) {
    console.log("transfer:", error?.name, error?.code ?? "", handle.fd === -1);
  }
  port1.postMessage("barrier");
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
