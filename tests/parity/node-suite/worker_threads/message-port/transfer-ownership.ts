import { MessageChannel, MessagePort, receiveMessageOnPort } from "node:worker_threads";

const movable = new MessageChannel();
const first = movable.port1;
movable.port2.postMessage("pending-one");
movable.port2.postMessage("pending-two");
const graph: any = { port: first, alias: first };
graph.self = graph;
graph.map = new Map([[first, graph]]);
graph.set = new Set([first]);
const buffer = new ArrayBuffer(8);
graph.buffer = buffer;
graph.view = new Uint8Array(buffer);
const copy = structuredClone(graph, { transfer: [first, buffer] });
console.log("graph", copy.port instanceof MessagePort, copy.port === copy.alias,
  copy.self === copy, copy.map.get(copy.port) === copy, copy.set.has(copy.port),
  copy.view.buffer === copy.buffer, buffer.byteLength);
console.log("fifo", receiveMessageOnPort(copy.port)?.message,
  receiveMessageOnPort(copy.port)?.message);
const latest = structuredClone(copy.port, { transfer: [copy.port] });
console.log("generations", first !== copy.port, copy.port !== latest, first !== latest,
  first.hasRef(), copy.port.hasRef());
first.postMessage("stale-one");
copy.port.postMessage("stale-two");
first.close();
copy.port.close();
latest.postMessage("current");
console.log("owner", receiveMessageOnPort(movable.port2)?.message,
  receiveMessageOnPort(movable.port2));
latest.close();
movable.port2.close();

const carrier = new MessageChannel();
const closing = new MessageChannel();
carrier.port1.postMessage({ port: closing.port1, alias: closing.port1 }, [closing.port1]);
closing.port2.close();
const adopted = receiveMessageOnPort(carrier.port2)!.message;
console.log("transit", adopted.port instanceof MessagePort,
  adopted.port === adopted.alias, adopted.port.hasRef());
let closes = 0;
adopted.port.on("close", () => { closes++; });
adopted.port.ref();
console.log("disentangled", receiveMessageOnPort(adopted.port),
  adopted.port.hasRef(), adopted.port.postMessage("inert"));
setTimeout(() => {
  console.log("transit close", closes, adopted.port.hasRef());
  adopted.port.close();
  carrier.port1.close();
  carrier.port2.close();
}, 30);
