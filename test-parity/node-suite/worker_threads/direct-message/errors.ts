import {
  Worker,
  postMessageToThread,
  threadId,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/direct-message");

async function logRejection(label: string, promise: Promise<any>) {
  try {
    await promise;
    console.log(label, "resolved");
  } catch (err: any) {
    console.log(label, err.name, err.code, err.message);
  }
}

async function run() {
  await logRejection("same-thread:", postMessageToThread(threadId, "self"));

  const worker = new Worker("./no-listener-worker.cjs");
  worker.on("message", async (message: any) => {
    if (message.phase !== "ready") {
      return;
    }

    await logRejection(
      "no-listener:",
      postMessageToThread(worker.threadId, "direct", undefined, 1000),
    );
    worker.terminate().then((code) => {
      console.log("terminate:", code);
    });
  });
}

run();
