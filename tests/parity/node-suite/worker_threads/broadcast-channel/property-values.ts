import { BroadcastChannel, MessageChannel } from "node:worker_threads";

const channel = new BroadcastChannel("property-values");
let calls = 0;
const handler = () => { calls++; };
channel.onmessage = handler;
console.log("name:", channel.name);
console.log("handler:", channel.onmessage === handler, calls);
console.log("method:", typeof channel.postMessage, typeof channel.close);
console.log("missing:", (channel as any).missing);
(channel as any).custom = 7;
console.log("custom:", (channel as any).custom);
(channel as any).invoke = function () { return this === channel; };
console.log("receiver:", (channel as any).invoke());
channel.onmessage!({} as MessageEvent);
console.log("called:", calls);
channel.onmessage = null;
console.log("cleared:", channel.onmessage);
channel.close();

const pair = new MessageChannel();
const port = pair.port1;
port.onmessage = handler;
console.log("port handler:", port.onmessage === handler, calls);
console.log("port method:", typeof port.postMessage, typeof port.close);
port.onmessage!({} as MessageEvent);
console.log("port called:", calls);
port.close();
pair.port2.close();
