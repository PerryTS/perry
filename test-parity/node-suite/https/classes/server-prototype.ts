import * as https from "node:https";

console.log(
  JSON.stringify(Object.getOwnPropertyNames(https.Server.prototype).sort()),
);
function logDescriptor(name: string) {
  const descriptor = Object.getOwnPropertyDescriptor(
    https.Server.prototype,
    name,
  );
  console.log(
    name,
    descriptor
      ? [
        typeof descriptor.value,
        descriptor.enumerable,
        descriptor.writable,
        descriptor.configurable,
      ].join(" ")
      : "missing",
  );
}

logDescriptor("close");
logDescriptor("closeAllConnections");
logDescriptor("closeIdleConnections");
logDescriptor("setTimeout");
