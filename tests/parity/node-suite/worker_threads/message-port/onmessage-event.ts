import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
port1.onmessage = (event) => {
  console.log(
    "event:",
    event.type,
    event.data.value,
    event.target === port1,
    event.currentTarget === port1,
    Array.isArray(event.ports) ? event.ports.length : "missing",
  );
  port1.close();
  port2.close();
};

port2.postMessage({ value: 7 });
