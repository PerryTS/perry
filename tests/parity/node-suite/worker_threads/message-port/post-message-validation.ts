import { MessageChannel } from "node:worker_threads";

function outcome(value: any, transfer?: any): string {
  try {
    if (arguments.length === 1) {
      port1.postMessage(value);
    } else {
      port1.postMessage(value, transfer);
    }
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const { port1, port2 } = new MessageChannel();
console.log("function:", outcome(() => {}));
console.log("symbol:", outcome(Symbol("value")));
console.log("number transfer:", outcome("value", 1));
console.log("false transfer:", outcome("value", false));
console.log("string transfer:", outcome("value", "bad"));
console.log("symbol transfer:", outcome("value", Symbol("bad")));
console.log("options transfer:", outcome("value", { transfer: null }));
console.log("zero options transfer:", outcome("value", { transfer: 0 }));
console.log("false options transfer:", outcome("value", { transfer: false }));
console.log("object options transfer:", outcome("value", { transfer: {} }));

port1.close();
port2.close();
