function method(label: string, proto: any, name: string): void {
  const value = proto[name];
  const descriptor = Object.getOwnPropertyDescriptor(proto, name);
  console.log(label, name, typeof value, value?.length, descriptor?.enumerable);
}

method("URL", URL.prototype, "toString");
method("URL", URL.prototype, "toJSON");
method("AbortController", AbortController.prototype, "abort");
method("AbortSignal", AbortSignal.prototype, "throwIfAborted");
method("EventTarget", EventTarget.prototype, "addEventListener");
method("EventTarget", EventTarget.prototype, "removeEventListener");
method("EventTarget", EventTarget.prototype, "dispatchEvent");
method("Event", Event.prototype, "initEvent");
method("Event", Event.prototype, "stopImmediatePropagation");
method("Event", Event.prototype, "preventDefault");
method("Event", Event.prototype, "composedPath");
method("Event", Event.prototype, "stopPropagation");
console.log("parents", Object.getPrototypeOf(AbortSignal.prototype) === EventTarget.prototype,
  Object.getPrototypeOf(CustomEvent.prototype) === Event.prototype);
console.log("inherited", typeof AbortSignal.prototype.addEventListener,
  typeof CustomEvent.prototype.preventDefault,
  Object.hasOwn(CustomEvent.prototype, "preventDefault"));

const url = new URL("https://example.com/a");
console.log("URL calls", URL.prototype.toString.call(url), URL.prototype.toJSON.call(url));

const target = new EventTarget();
let called = 0;
const listener = () => { called++; };
EventTarget.prototype.addEventListener.call(target, "hit", listener);
console.log("dispatch", EventTarget.prototype.dispatchEvent.call(target, new Event("hit")), called);
EventTarget.prototype.removeEventListener.call(target, "hit", listener);
EventTarget.prototype.dispatchEvent.call(target, new Event("hit"));
console.log("removed", called);
for (const [name, invoke] of [
  ["add", () => EventTarget.prototype.addEventListener.call({}, "x", listener)],
  ["remove", () => EventTarget.prototype.removeEventListener.call({}, "x", listener)],
  ["dispatch", () => EventTarget.prototype.dispatchEvent.call({}, new Event("x"))],
] as const) {
  try { invoke(); } catch (error: any) {
    console.log("invalid target", name, error.name, error.message);
  }
}

const event = new Event("x", { cancelable: true });
Event.prototype.preventDefault.call(event);
console.log("prevented", event.defaultPrevented);
Event.prototype.initEvent.call(event, "changed", true, false);
console.log("initialized", event.type, event.bubbles, event.cancelable,
  Event.prototype.composedPath.call(event).length);
const custom = new CustomEvent("x", { cancelable: true });
CustomEvent.prototype.preventDefault.call(custom);
console.log("custom prevented", custom.defaultPrevented);

const controller = new AbortController();
AbortSignal.prototype.throwIfAborted.call(controller.signal);
AbortController.prototype.abort.call(controller);
console.log("aborted", controller.signal.aborted);
