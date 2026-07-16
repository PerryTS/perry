import {
  MessageChannel,
  receiveMessageOnPort,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const channel = new MessageChannel();
try {
  const worker = new Worker("./workerdata-port-container-worker.cjs", {
    workerData: {
      kind: "map",
      container: new Map([["port", channel.port1]]),
    },
    transferList: [channel.port1],
  });
  worker.on("message", (message) => {
    console.log(
      "map:",
      message.kind,
      message.containerBrand,
      message.portBrand,
      message.extractionError,
    );
    console.log("delivery:", receiveMessageOnPort(channel.port2)?.message);
  });
  worker.on(
    "error",
    (error) => console.log("error:", error.name, (error as any).code ?? ""),
  );
  worker.on("exit", (code) => {
    console.log("exit:", code);
    channel.port1.close();
    channel.port2.close();
  });
} catch (error) {
  console.log("construct:", (error as any)?.name, (error as any)?.code ?? "");
  channel.port1.close();
  channel.port2.close();
}
