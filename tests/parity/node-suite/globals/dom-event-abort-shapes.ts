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

const plain = new Event("plain");
plain.preventDefault();
show(
  "plain prevent default",
  plain.cancelable,
  plain.defaultPrevented,
  target.dispatchEvent(plain),
);

const removableTarget = new EventTarget();
const removableOrder: string[] = [];
function removable() {
  removableOrder.push("hit");
}
removableTarget.addEventListener("remove", removable);
removableTarget.addEventListener("remove", removable);
removableTarget.dispatchEvent(new Event("remove"));
removableTarget.removeEventListener("remove", removable);
removableTarget.dispatchEvent(new Event("remove"));
show("dedupe remove", removableOrder.join("|"));

const captureTarget = new EventTarget();
const captureOrder: string[] = [];
function captured() {
  captureOrder.push("hit");
}
captureTarget.addEventListener("capture", captured, true);
captureTarget.addEventListener("capture", captured, false);
captureTarget.dispatchEvent(new Event("capture"));
captureTarget.removeEventListener("capture", captured, false);
captureTarget.dispatchEvent(new Event("capture"));
show("capture remove", captureOrder.join("|"));

const onceTarget = new EventTarget();
const onceOrder: string[] = [];
onceTarget.addEventListener("once", () => onceOrder.push("once"), {
  once: true,
});
onceTarget.dispatchEvent(new Event("once"));
onceTarget.dispatchEvent(new Event("once"));
show("once option", onceOrder.join("|"));

const signaledTarget = new EventTarget();
const signalOrder: string[] = [];
const listenerSignal = new AbortController();
signaledTarget.addEventListener("signal", () => signalOrder.push("live"), {
  signal: listenerSignal.signal,
});
listenerSignal.abort();
signaledTarget.dispatchEvent(new Event("signal"));
const alreadyAborted = AbortSignal.abort();
signaledTarget.addEventListener("signal", () => signalOrder.push("pre"), {
  signal: alreadyAborted,
});
signaledTarget.dispatchEvent(new Event("signal"));
show("signal option", signalOrder.join("|"));

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

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    show(label, "ok");
  } catch (error) {
    show(label, (error as any).name, (error as any).code);
  }
}

showError("event missing type", () => new (Event as any)());
showError("custom missing type", () => new (CustomEvent as any)());
showError("dispatch missing event", () =>
  (new EventTarget() as any).dispatchEvent(),
);
showError("dispatch plain object", () =>
  new EventTarget().dispatchEvent({ type: "plain" } as any),
);
