const wt = require("node:worker_threads");
const bare = require("worker_threads");
const { parentPort, receiveMessageOnPort: receive, postMessageToThread: send } = wt;
const rows = [];
parentPort.once("message", () => parentPort.close());
const record = (label, action) => {
  try { rows.push(label + " " + JSON.stringify(action())); }
  catch (error) { rows.push(label + " " + error.name + ":" + (error.code ?? "")); }
};

async function main() {
  record("CJS identity", () => receive === bare.receiveMessageOnPort);
  const { port1, port2 } = new wt.MessageChannel();
  for (const [label, invoke] of [
    ["captured", (port) => receive(port)],
    ["call", (port) => receive.call({}, port)],
    ["apply", (port) => receive.apply(null, [port])],
    ["spread", (port) => receive(...[port, "ignored"])],
  ]) {
    port1.postMessage(label);
    record("CJS " + label, () => invoke(port2).message);
  }
  record("CJS empty", () => receive(port2) === undefined);
  record("CJS missing", () => receive());
  record("CJS invalid", () => receive({}));
  port1.close(); port2.close();
  record("CJS closed", () => receive(port2) === undefined);
  const replacement = () => "replacement";
  wt.receiveMessageOnPort = replacement;
  record("CJS override read", () => wt.receiveMessageOnPort === replacement);
  wt.receiveMessageOnPort = receive;
  record("CJS restored", () => wt.receiveMessageOnPort === receive);
  for (const [label, invoke] of [
    ["captured", () => send(999999, "message")],
    ["call", () => send.call(null, 999999, "message")],
    ["apply", () => send.apply({}, [999999, "message"])],
    ["spread", () => send(...[999999, "message", undefined, 10, "ignored"])],
  ]) {
    try {
      const result = invoke();
      rows.push("CJS " + label + " promise " + (result instanceof Promise));
      await result;
    } catch (error) {
      rows.push("CJS " + label + " rejection " + error.name + ":" + (error.code ?? ""));
    }
  }
  parentPort.postMessage(rows);
}
main();
