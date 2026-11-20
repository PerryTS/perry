import { MessageChannel } from "node:worker_threads";

const first = new MessageChannel();
const second = new MessageChannel();
let iterations = 0;

const ports = {
  *[Symbol.iterator]() {
    iterations += 1;
    yield first.port1;
    yield second.port1;
  },
};

const event = new MessageEvent("message", { ports: ports as any });
console.log(
  "iterable:",
  iterations,
  event.ports?.length ?? "missing",
  event.ports?.[0] === first.port1,
  event.ports?.[1] === second.port1,
);

try {
  new MessageEvent("message", {
    ports: new Set([first.port2, {} as MessagePort]) as any,
  });
  console.log("invalid iterable: ok");
} catch (error: any) {
  console.log("invalid iterable:", error?.name, error?.code ?? "");
}

first.port1.close();
first.port2.close();
second.port1.close();
second.port2.close();
