import { Worker } from "node:worker_threads";

function check(name: string, method: Function, args: any[] = []) {
  for (const receiver of [undefined, null, {}]) {
    try {
      Reflect.apply(method, receiver, args);
      console.log(name, typeof receiver, "ok");
    } catch (error: any) {
      console.log(name, typeof receiver, error?.name, error?.code ?? "");
    }
  }
}

check("postMessage", Worker.prototype.postMessage, ["value"]);
check("terminate", Worker.prototype.terminate);
check("ref", Worker.prototype.ref);
check("unref", Worker.prototype.unref);
