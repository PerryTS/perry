import { MessageChannel } from "node:worker_threads";

function boom(name: string) {
  return function () {
    throw new Error(`tampered ${name}`);
  };
}

for (
  const [constructor, names] of [
    [Map, [
      "get",
      "set",
      "delete",
      "has",
      "values",
      "keys",
      "entries",
      "forEach",
    ]],
    [Set, ["add", "delete", "has", "values", "keys", "entries", "forEach"]],
    [WeakMap, ["get", "set", "has", "delete"]],
  ] as const
) {
  for (const name of names) {
    (constructor.prototype as any)[name] = boom(`${constructor.name}.${name}`);
  }
  if (constructor !== WeakMap) {
    Object.defineProperty(constructor.prototype, "size", {
      configurable: true,
      get: boom(`${constructor.name}.size`),
    });
  }
  (constructor.prototype as any)[Symbol.iterator] = boom(
    `${constructor.name}[Symbol.iterator]`,
  );
}

try {
  const { port1, port2 } = new MessageChannel();
  const listener = () => {};
  port1.on("message", listener);
  const before = (port1 as any).listenerCount("message");
  port1.once("close", () => {});
  const names = (port1 as any).eventNames().sort().join(",");
  port1.off("message", listener);
  const after = (port1 as any).listenerCount("message");
  port1.removeAllListeners();
  const empty = (port1 as any).eventNames().length;
  console.log("registry:", before, names, after, empty);
  port1.close();
  port2.close();
} catch (error: any) {
  console.log("registry error:", error?.name, error?.message);
}
