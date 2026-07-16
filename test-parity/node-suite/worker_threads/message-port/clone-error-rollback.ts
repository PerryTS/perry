import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);

try {
  port1.postMessage({ buffer, invalid() {} }, [buffer]);
  console.log("post: ok");
} catch (error: any) {
  console.log("post:", error?.name, error?.code ?? "");
}

console.log("buffer retained:", buffer.byteLength);
console.log("nothing queued:", receiveMessageOnPort(port2));
port1.close();
port2.close();
