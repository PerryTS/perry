import { setImmediate } from "node:timers";

function probe(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, err?.name, err?.code);
  }
}

probe("missing", () => queueMicrotask());
probe("undefined", () => queueMicrotask(undefined as any));
probe("number", () => queueMicrotask(1 as any));
probe("object", () => queueMicrotask({} as any));

const order: string[] = [];
queueMicrotask(() => order.push("micro"));
order.push("sync");

await new Promise<void>((resolve) => {
  setImmediate(() => {
    console.log("order:", order.join(","));
    resolve();
  });
});
