import { open } from "node:fs/promises";
import { MessageChannel } from "node:worker_threads";

const fixture =
  "test-parity/node-suite/worker_threads/worker-lifecycle/process-env-option-worker.cjs";

async function main() {
  const handle = await open(fixture, "r");
  const clone = new MessageChannel();

  try {
    try {
      clone.port1.postMessage(handle);
      console.log("clone: accepted", handle.fd >= 0);
    } catch (error: any) {
      console.log(
        "clone:",
        error?.name,
        error?.code ?? "",
        handle.fd >= 0,
      );
    } finally {
      clone.port1.close();
      clone.port2.close();
    }

    const transfer = new MessageChannel();
    const received = new Promise<void>((resolve) => {
      transfer.port2.on("message", async (value: any) => {
        console.log(
          "received:",
          value?.constructor?.name,
          typeof value?.fd === "number" && value.fd >= 0,
        );
        try {
          const text = await value.readFile({ encoding: "utf8" });
          console.log("received read:", text.startsWith("const { parentPort"));
        } catch (error: any) {
          console.log("received read:", error?.name, error?.code ?? "");
        }
        await value?.close?.().catch?.(() => {});
        try {
          await handle.readFile();
          console.log("parent read: ok");
        } catch (error: any) {
          console.log("parent read:", error?.name, error?.code ?? "");
        }
        transfer.port1.close();
        transfer.port2.close();
        resolve();
      });
    });

    try {
      transfer.port1.postMessage(handle, [handle as any]);
      console.log("parent detached:", handle.fd === -1);
      await received;
    } catch (error: any) {
      console.log(
        "transfer:",
        error?.name,
        error?.code ?? "",
        handle.fd === -1,
      );
      transfer.port1.close();
      transfer.port2.close();
    }
  } finally {
    await handle.close().catch(() => {});
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
