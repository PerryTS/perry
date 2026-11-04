import {
  MessageChannel, Worker, getEnvironmentData, setEnvironmentData,
  markAsUncloneable, markAsUntransferable, isMarkedAsUntransferable,
  receiveMessageOnPort, postMessageToThread,
} from "node:worker_threads";

function record(label: string, action: () => any): void {
  try {
    console.log(label, action());
  } catch (error: any) {
    console.log(label, error.name, error.code ?? "");
  }
}

async function main(): Promise<void> {
  const get = getEnvironmentData;
  const set = setEnvironmentData;
  setEnvironmentData("direct", 17);
  record("named environment", () => {
    set.call(null, "captured", 31, "ignored");
    return get.apply(null, ["captured"]) === 31 && get("direct") === 17;
  });
  record("named missing", () => {
    set(undefined, 32);
    const present = get() === 32;
    set();
    return present && get(undefined) === undefined;
  });
  const untransferable = markAsUntransferable;
  const uncloneable = markAsUncloneable;
  const marked = isMarkedAsUntransferable;
  record("named marker", () => {
    const value = {};
    untransferable.apply(null, [value]);
    return marked.call(null, value) && !marked();
  });
  record("named uncloneable", () => {
    const value = {};
    uncloneable(value);
    try { structuredClone(value); } catch (error: any) { return `${error.name}:${error.code}`; }
  });
  const { port1, port2 } = new MessageChannel();
  const receive = receiveMessageOnPort;
  port1.postMessage("queued");
  record("named receive", () => receive.call(null, port2)?.message);
  record("named empty", () => receive.apply(null, [port2]) === undefined);
  port1.close();
  port2.close();
  const send = postMessageToThread;
  try {
    const result = send(...[999999, "message"]);
    console.log("named promise", result instanceof Promise);
    await result;
  } catch (error: any) {
    console.log("named rejection", error.name, error.code ?? "");
  }
  const worker = new Worker(new URL("./fixtures/callable-exports-worker.cjs", import.meta.url));
  worker.on("message", (rows) => {
    rows.forEach((row) => console.log(row));
    worker.postMessage("done");
  });
  worker.on("exit", (code) => console.log("worker exit", code));
}
main();
