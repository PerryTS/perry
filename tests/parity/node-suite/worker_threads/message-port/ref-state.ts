import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
console.log("initial:", port1.hasRef());

const listener = () => {};
port1.on("message", listener);
console.log("with listener:", port1.hasRef());
console.log("unref return:", port1.unref() === port1, port1.hasRef());
console.log("ref return:", port1.ref() === port1, port1.hasRef());

port1.off("message", listener);
console.log("without listener:", port1.hasRef());
port1.close();
port2.close();
