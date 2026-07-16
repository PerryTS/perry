import {
  MessageChannel,
  postMessageToThread,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/direct-message");

const worker = new Worker("./transferables-worker.cjs");
const { port1, port2 } = new MessageChannel();
const buffer = new Uint8Array([2, 4, 6, 8]).buffer;

worker.once("message", async () => {
  try {
    await postMessageToThread(
      worker.threadId,
      { buffer, port: port2 },
      [buffer, port2],
    );
    const delivered = receiveMessageOnPort(port1)?.message;
    console.log(
      "delivered:",
      delivered?.portBrand,
      delivered?.bufferBrand,
      delivered?.values,
      delivered?.source === 0,
    );
    console.log("ownership:", buffer.byteLength, true);
  } catch (error: any) {
    console.log("direct:", error?.name, error?.code ?? "");
  }
  port1.close();
  port2.close();
  worker.terminate().then((code) => console.log("terminate:", code));
});
