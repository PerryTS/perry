import { Worker } from "node:worker_threads";

async function main() {
  const order: string[] = [];
  process.nextTick(() => order.push("before-tick"));
  const first = new Worker("", { eval: true });
  order.push("constructed");
  let sameWorker = false;
  process.once("worker", (created) => {
    sameWorker = created === first;
    order.push("worker-event");
  });
  process.nextTick(() => order.push("after-tick"));
  queueMicrotask(() => order.push("microtask"));
  await new Promise<void>((resolve) => first.once("online", () => {
    order.push("online");
    resolve();
  }));
  console.log("default:", order.join(","), sameWorker);
  await first.terminate();

  const nextTick = process.nextTick;
  const emit = process.emit;
  const held: any[] = [];
  (process as any).nextTick = function (callback: any, ...args: any[]) {
    held.push({ callback, args, receiver: this });
  };
  const second = new Worker("", { eval: true });
  process.nextTick = nextTick;
  let notifications = 0;
  let getterReads = 0;
  let emitterThis = false;
  let identity = false;
  const sentinel = {};
  Object.defineProperty(process, "emit", {
    configurable: true,
    get() {
      getterReads++;
      return function (event: string, worker: Worker) {
        if (event === "worker") {
          notifications++;
          emitterThis = this === process;
          identity = worker === second;
          return sentinel;
        }
        return Reflect.apply(emit, this, arguments as any);
      };
    },
  });
  console.log("held before:", notifications);
  let contract = "missing";
  for (const job of held) {
    const before = notifications;
    const result = Reflect.apply(job.callback, { alternate: true }, job.args);
    if (notifications !== before) {
      let constructible = true;
      try { Reflect.construct(function () {}, [], job.callback); }
      catch (error: any) { constructible = error.name !== "TypeError"; }
      contract = [
        job.receiver === process, job.args.length === 0,
        job.callback.name === "", job.callback.length === 0,
        !constructible, result === sentinel,
      ].join(",");
    }
  }
  Object.defineProperty(process, "emit", { configurable: true, writable: true, value: emit });
  console.log("held contract:", contract);
  console.log("late emit:", getterReads, notifications, emitterThis, identity);
  await second.terminate();
}

main().catch((error) => console.log("unexpected:", error.name, error.message));
