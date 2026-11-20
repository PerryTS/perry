import { BroadcastChannel, MessageChannel, Worker, receiveMessageOnPort } from "node:worker_threads";

function check(label: string, call: () => void) {
  try {
    call();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code ?? "", error.code === "ERR_MISSING_ARGS" ? error.message : "");
  }
}

const { port1, port2 } = new MessageChannel();
const post = port1.postMessage;
check("port direct:", () => port1.postMessage());
check("port spread:", () => port1.postMessage(...[]));
check("port extracted:", () => post.call(port1));
check("port receiver:", () => post.call({}));
console.log("port rejected queue:", receiveMessageOnPort(port2) === undefined);
port1.postMessage(undefined);
const packet = receiveMessageOnPort(port2);
console.log("port undefined:", packet !== undefined, typeof packet?.message);
port1.close();
port2.close();

const sender = new BroadcastChannel("argument-presence");
const receiver = new BroadcastChannel("argument-presence");
check("broadcast missing:", () => sender.postMessage());
console.log("broadcast rejected queue:", receiveMessageOnPort(receiver) === undefined);
sender.postMessage(undefined);
const broadcastPacket = receiveMessageOnPort(receiver);
console.log("broadcast undefined:", broadcastPacket !== undefined, typeof broadcastPacket?.message);
sender.close();
check("broadcast closed missing:", () => sender.postMessage());
check("broadcast closed undefined:", () => sender.postMessage(undefined));
receiver.close();

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");
const worker = new Worker("./postmessage-presence-worker.cjs");
worker.on("online", () => {
  const original = worker.postMessage;
  check("worker direct:", () => worker.postMessage());
  check("worker spread:", () => worker.postMessage(...[]));
  check("worker extracted:", () => original.call(worker));
  (worker as any).postMessage = (value: any) => console.log("worker override:", String(value));
  worker.postMessage();
  let reads = 0;
  Object.defineProperty(worker, "postMessage", {
    configurable: true,
    get() { reads++; return original; },
  });
  check("worker getter:", () => worker.postMessage());
  console.log("worker getter reads:", reads);
  Object.defineProperty(worker, "postMessage", { configurable: true, writable: true, value: original });
  worker.postMessage(undefined);
  worker.postMessage("finish");
});
worker.on("message", (message) => console.log("worker message:", message));
worker.on("exit", (code) => console.log("worker exit:", code));
