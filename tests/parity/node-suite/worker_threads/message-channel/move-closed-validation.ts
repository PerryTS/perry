import * as vm from "node:vm";
import {
  MessageChannel,
  MessagePort,
  moveMessagePortToContext,
} from "node:worker_threads";

function outcome(fn: () => any): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const closed = new MessageChannel();
closed.port1.close();
const context = vm.createContext({ MessagePort });

console.log(
  "closed:",
  outcome(() => moveMessagePortToContext(closed.port1, context)),
);
console.log(
  "invalid port:",
  outcome(() => moveMessagePortToContext({} as any, context)),
);
console.log(
  "invalid context:",
  outcome(() => moveMessagePortToContext(closed.port2, {} as any)),
);

closed.port2.close();
