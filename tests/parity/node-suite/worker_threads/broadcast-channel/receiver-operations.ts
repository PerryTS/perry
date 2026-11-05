import { BroadcastChannel } from "node:worker_threads";

function outcome(action: () => any): string {
  try {
    action();
    return "ok";
  } catch (error: any) {
    return `${error.name}:${error.code ?? ""}`;
  }
}
const channels = ["a", "b", "a", "b", "proxy", "name"].map((name) => new BroadcastChannel(name));
const [a, b, sinkA, sinkB, proxyTarget, named] = channels;
const prototype = BroadcastChannel.prototype as any;
const descriptor = Object.getOwnPropertyDescriptor(prototype, "name");
const getter = descriptor?.get as any;

async function delivered(receiver: BroadcastChannel, send: () => void): Promise<any> {
  const message = new Promise((resolve) => { receiver.onmessage = (event) => resolve(event.data); });
  send();
  return await message;
}
async function main() {
  console.log("subjects", channels.length);
  console.log("descriptor", typeof getter, typeof descriptor?.set, descriptor?.enumerable, descriptor?.configurable);
  if (typeof getter !== "function") {
    console.log("missing prototype getter");
    return;
  }
  console.log("name", Object.hasOwn(named, "name"), Reflect.set(named, "name", "replacement"), named.name);
  console.log("assignment", outcome(() => { (named as any).name = "replacement"; }), named.name);
  console.log("prototype post", await delivered(sinkA, () => prototype.postMessage.call(a, "prototype")));
  console.log("borrowed post", await delivered(sinkB, () => a.postMessage.call(b, "borrowed")));
  console.log("refs", a.ref.call(b) === b, a.unref.call(b) === b, prototype.ref.call(b) === b);
  a.close.call(b);
  console.log("borrowed close", outcome(() => b.postMessage(0)), await delivered(sinkA, () => a.postMessage("alive")));
  console.log("closed name", getter.call(b), outcome(() => prototype.close.call(b)));
  const proxy = new Proxy(proxyTarget, {});
  console.log("proxy", getter.call(proxy), prototype.ref.call(proxy) === proxy, prototype.unref.call(proxy) === proxy);
  prototype.close.call(proxy);
  console.log("proxy closed", outcome(() => proxyTarget.postMessage(0)));

  for (const [label, receiver] of [
    ["undefined", undefined], ["null", null], ["number", 1], ["string", "a"],
    ["plain", {}], ["prototype", prototype], ["inherited", Object.create(prototype)],
    ["impostor", { name: "a", closed: false, id: 1 }],
  ] as const) {
    console.log("invalid", label, outcome(() => getter.call(receiver)),
      ...["postMessage", "close", "ref", "unref"].map((method) => outcome(() => prototype[method].call(receiver))));
  }
  let clones = 0;
  const payload = { get value() { clones++; return 7; } };
  console.log("invalid precedence", outcome(() => prototype.postMessage.call({}, payload)), clones);
  console.log("closed missing", outcome(() => prototype.postMessage.call(b)));
  console.log("closed clone", outcome(() => prototype.postMessage.call(b, payload)), clones);
  console.log("closed symbol", outcome(() => prototype.postMessage.call(b, Symbol("data"))));
  console.log("open missing", outcome(() => prototype.postMessage.call(a)));
  console.log("open symbol", outcome(() => prototype.postMessage.call(a, Symbol("data"))));
  await delivered(sinkA, () => prototype.postMessage.call(a, undefined));
  console.log("undefined delivered");
  const cloned = await delivered(sinkA, () => prototype.postMessage.call(a, payload));
  console.log("clone", cloned.value, clones);
  const original = globalThis.BroadcastChannel;
  try {
    (globalThis as any).BroadcastChannel = function Replacement() {};
    const captured = new BroadcastChannel("canonical");
    console.log("global replacement", captured.name, typeof captured.close, prototype.ref.call(captured) === captured);
    prototype.close.call(captured);
  } finally {
    globalThis.BroadcastChannel = original;
  }
  const derived = Object.create(named);
  console.log("derived", derived.name, prototype.ref.call(derived) === derived, prototype.unref.call(derived) === derived);
  prototype.close.call(derived);
  console.log("derived close", outcome(() => prototype.postMessage.call(derived, 0)));
  console.log("retained name", getter.call(named));
  console.log("complete", channels.length);
}
main().then(() => { for (const channel of channels) channel.close(); }, (error) => {
  for (const channel of channels) channel.close();
  throw error;
});
