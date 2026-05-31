function show(label: string, ...values: unknown[]) {
  console.log(label + ":", values.map(String).join(" "));
}

show(
  "Event ctor",
  Event.name,
  Event.length,
  typeof Event.prototype,
  globalThis.Event === Event,
);
show(
  "CustomEvent ctor",
  CustomEvent.name,
  CustomEvent.length,
  typeof CustomEvent.prototype,
  globalThis.CustomEvent === CustomEvent,
);
show(
  "DOMException ctor",
  DOMException.name,
  DOMException.length,
  typeof DOMException.prototype,
  globalThis.DOMException === DOMException,
);

const event = new Event("alpha", {
  bubbles: true,
  cancelable: true,
  composed: true,
});
show(
  "event options",
  event.type,
  event.bubbles,
  event.cancelable,
  event.composed,
  event.defaultPrevented,
  event.constructor === Event,
  event instanceof Event,
);

const target = new EventTarget();
const order: string[] = [];
target.addEventListener("alpha", (seen: Event) => {
  order.push(
    [
      "first",
      seen.type,
      seen.target === target,
      seen.currentTarget === target,
      seen.eventPhase,
    ].join("/"),
  );
  seen.preventDefault();
});
target.addEventListener("alpha", () => {
  order.push("second");
});

const dispatchResult = target.dispatchEvent(event);
show(
  "dispatch",
  dispatchResult,
  event.defaultPrevented,
  event.target === target,
  event.currentTarget === null,
  event.eventPhase,
);
show("listener order", order.join("|"));

const custom = new CustomEvent("beta", {
  detail: { answer: 42 },
  cancelable: true,
});
show(
  "custom event",
  custom.type,
  (custom.detail as any).answer,
  custom.cancelable,
  custom.constructor === CustomEvent,
  custom instanceof Event,
  custom instanceof CustomEvent,
);

const cloneError = new DOMException("bad", "DataCloneError");
show(
  "dom exception",
  cloneError.name,
  cloneError.message,
  cloneError.code,
  cloneError.constructor === DOMException,
  cloneError instanceof Error,
  cloneError instanceof DOMException,
);

const controller = new AbortController();
controller.abort();
const reason = controller.signal.reason as DOMException;
show(
  "abort default",
  controller.signal.aborted,
  reason.name,
  reason.message,
  reason.code,
  reason.constructor === DOMException,
  reason instanceof DOMException,
);
