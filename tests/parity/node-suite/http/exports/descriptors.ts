import http from "node:http";

for (
  const name of [
    "globalAgent",
    "maxHeaderSize",
    "WebSocket",
    "CloseEvent",
    "MessageEvent",
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(http, name)!;
  console.log(
    name,
    [
      descriptor.enumerable,
      descriptor.configurable,
      typeof descriptor.get,
      typeof descriptor.set,
      "value" in descriptor,
    ].join("|"),
  );
}

console.log("frozen:", Object.isFrozen(http));
console.log("tag:", Object.prototype.toString.call(http));
