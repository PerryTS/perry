import { Worker, postMessageToThread } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/direct-message");

const worker = new Worker("./worker.cjs");
let directMessage: any;
let resolved = false;

function maybeFinish() {
  if (!directMessage || !resolved) {
    return;
  }

  console.log(
    "direct:",
    directMessage.text,
    directMessage.count,
    directMessage.nestedOk,
    directMessage.source,
  );
  console.log("resolved:", resolved);
  worker.terminate().then((code) => {
    console.log("terminate:", code);
  });
}

worker.on("message", async (message: any) => {
  if (message.phase === "ready") {
    try {
      await postMessageToThread(
        worker.threadId,
        { text: "hello", count: 2, nested: { ok: true } },
        undefined,
        1000,
      );
      resolved = true;
      maybeFinish();
    } catch (err: any) {
      console.log("direct error:", err.code, err.message);
      worker.terminate();
    }
    return;
  }

  if (message.phase === "direct") {
    directMessage = message;
    maybeFinish();
  }
});
