function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

function dataDescriptor(desc: PropertyDescriptor | undefined) {
  if (!desc) return "missing";
  let value = desc.value;
  if (typeof value === "function") {
    value = value === WebSocket ? "WebSocket" : value.name;
  }
  return JSON.stringify({
    value,
    writable: desc.writable,
    enumerable: desc.enumerable,
    configurable: desc.configurable,
  });
}

const GlobalWebSocket = globalThis.WebSocket;
const AliasWebSocket = globalThis.WebSocket;

show("typeof WebSocket", typeof WebSocket);
show("typeof globalThis.WebSocket", typeof globalThis.WebSocket);
show("global identity", GlobalWebSocket === WebSocket);
show("alias identity", AliasWebSocket === WebSocket);
show("constructor name", WebSocket.name);
show("constructor length", WebSocket.length);
show("global constructor name", globalThis.WebSocket.name);
show("global constructor length", globalThis.WebSocket.length);

for (const key of ["CONNECTING", "OPEN", "CLOSING", "CLOSED"] as const) {
  show("static " + key, WebSocket[key]);
  show("global static " + key, globalThis.WebSocket[key]);
  show("prototype " + key, WebSocket.prototype[key]);
}

show(
  "static OPEN desc",
  dataDescriptor(Object.getOwnPropertyDescriptor(WebSocket, "OPEN")),
);
show(
  "prototype OPEN desc",
  dataDescriptor(Object.getOwnPropertyDescriptor(WebSocket.prototype, "OPEN")),
);
show("prototype constructor identity", WebSocket.prototype.constructor === WebSocket);
show(
  "prototype constructor desc",
  dataDescriptor(Object.getOwnPropertyDescriptor(WebSocket.prototype, "constructor")),
);

for (const method of ["send", "close"] as const) {
  const fn = WebSocket.prototype[method];
  show(method + " value", typeof fn + ":" + fn.name + ":" + fn.length);
  show(
    method + " desc",
    dataDescriptor(Object.getOwnPropertyDescriptor(WebSocket.prototype, method)),
  );
  show(
    method + " name desc",
    dataDescriptor(Object.getOwnPropertyDescriptor(fn, "name")),
  );
  show(
    method + " length desc",
    dataDescriptor(Object.getOwnPropertyDescriptor(fn, "length")),
  );
}
