function probe(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "no-throw");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

probe("missing", () => queueMicrotask());
probe("undefined", () => queueMicrotask(undefined as any));
probe("number", () => queueMicrotask(1 as any));
probe("object", () => queueMicrotask({} as any));
probe("null", () => queueMicrotask(null as any));

const order: string[] = [];
queueMicrotask(() => order.push("micro"));
order.push("sync");

await Promise.resolve();
console.log("order:", order.join(","));
