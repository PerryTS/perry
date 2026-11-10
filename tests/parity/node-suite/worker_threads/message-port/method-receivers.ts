import { MessageChannel, MessagePort } from "node:worker_threads";

function check(name: string, method: Function, args: any[] = []) {
  for (const receiver of [undefined, null, {}, new EventTarget()]) {
    try {
      Reflect.apply(method, receiver, args);
      console.log(name, typeof receiver, "ok");
    } catch (error: any) {
      console.log(name, typeof receiver, error?.name, error?.code ?? "");
    }
  }
}

check("postMessage", MessagePort.prototype.postMessage, ["value"]);
check("start", MessagePort.prototype.start);
check("close", MessagePort.prototype.close);
check("ref", MessagePort.prototype.ref);
check("unref", MessagePort.prototype.unref);
check("hasRef", MessagePort.prototype.hasRef);

const { port1, port2 } = new MessageChannel();
port1.close();
port2.close();
