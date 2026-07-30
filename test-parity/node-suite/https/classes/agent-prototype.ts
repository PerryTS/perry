import * as https from "node:https";

console.log(
  JSON.stringify(Object.getOwnPropertyNames(https.Agent.prototype).sort()),
);
function logDescriptor(name: string) {
  const descriptor = Object.getOwnPropertyDescriptor(
    https.Agent.prototype,
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

logDescriptor("createConnection");
logDescriptor("getName");
logDescriptor("_getSession");
logDescriptor("_cacheSession");
logDescriptor("_evictSession");
