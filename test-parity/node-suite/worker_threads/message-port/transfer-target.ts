import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);
new Uint8Array(buffer)[0] = 5;

try {
  port2.postMessage({ port: port1, buffer }, [port1, buffer]);
  console.log("post: ok");
} catch (error: any) {
  console.log("post:", error?.name, error?.code ?? "");
}

console.log("buffer detached:", buffer.byteLength);
console.log("target receives:", receiveMessageOnPort(port2));
port1.close();
port2.close();
