import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const getMaxListeners = (port1 as any).getMaxListeners;
const setMaxListeners = (port1 as any).setMaxListeners;

console.log("methods:", typeof getMaxListeners, typeof setMaxListeners);
if (
  typeof getMaxListeners === "function" &&
  typeof setMaxListeners === "function"
) {
  console.log("initial max:", getMaxListeners.call(port1));
  console.log("set return:", setMaxListeners.call(port1, 3));
  console.log("updated max:", getMaxListeners.call(port1));
}

const messageListener = () => {};
const customListener = () => {};
port1.on("message", messageListener);
port1.on("custom", customListener);

const removeAllListeners = (port1 as any).removeAllListeners;
console.log(
  "remove return:",
  typeof removeAllListeners === "function"
    ? removeAllListeners.call(port1, "message") === port1
    : "unsupported",
);
console.log(
  "remaining:",
  typeof (port1 as any).listenerCount === "function"
    ? (port1 as any).listenerCount("message")
    : "unsupported",
  typeof (port1 as any).listenerCount === "function"
    ? (port1 as any).listenerCount("custom")
    : "unsupported",
);

function invalid(label: string, value: any) {
  try {
    if (typeof setMaxListeners !== "function") {
      throw new TypeError("unsupported");
    }
    setMaxListeners.call(port1, value);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

invalid("negative:", -1);
invalid("string:", "3");
invalid("nan:", Number.NaN);

port1.close();
port2.close();
