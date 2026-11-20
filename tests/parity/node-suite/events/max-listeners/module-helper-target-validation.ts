import {
  EventEmitter,
  getEventListeners,
  getMaxListeners,
  listenerCount,
  setMaxListeners,
} from "node:events";

function show(label: string, fn: () => any): void {
  try {
    console.log(label, "OK", String(fn()));
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

const emitter = new EventEmitter();
function listener(): void {}
emitter.on("x", listener);

const target = new EventTarget();
target.addEventListener("x", listener);

const invalidTargets: Array<[string, any]> = [
  ["number", 123],
  ["string", "x"],
  ["object", {}],
  ["array", []],
];

for (const [label, target] of invalidTargets) {
  show(`getEventListeners ${label}`, () => getEventListeners(target, "x").length);
  show(`listenerCount ${label}`, () => listenerCount(target, "x"));
  show(`getMaxListeners ${label}`, () => getMaxListeners(target));
  show(`setMaxListeners ${label}`, () => setMaxListeners(2, target));
}

show("getEventListeners emitter", () => getEventListeners(emitter, "x").length);
show("listenerCount emitter", () => listenerCount(emitter, "x"));
show("getMaxListeners emitter", () => getMaxListeners(emitter));
show("setMaxListeners emitter", () => setMaxListeners(3, emitter));

show("getEventListeners eventtarget", () => getEventListeners(target, "x").length);
show("listenerCount eventtarget", () => listenerCount(target, "x"));
show("getMaxListeners eventtarget", () => getMaxListeners(target));
show("setMaxListeners eventtarget", () => setMaxListeners(4, target));
show("setMaxListeners no targets", () => setMaxListeners(5));
