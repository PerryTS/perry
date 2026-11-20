import { EventEmitter, on } from "node:events";

function show(label: string, fn: () => any): void {
  try {
    const value = fn();
    console.log(label, "OK", typeof value);
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

const emitter = new EventEmitter();
const signal = new AbortController().signal;

show("number emitter", () => on(123 as any, "x"));
show("object emitter", () => on({} as any, "x"));
show("number options", () => on(emitter, "x", 1 as any));
show("null options", () => on(emitter, "x", null as any));
show("object signal", () => on(emitter, "x", { signal: {} as any }));
show("null signal", () => on(emitter, "x", { signal: null as any }));
show("valid signal", () => on(emitter, "x", { signal }));
show("valid eventtarget", () => on(new EventTarget(), "x"));
