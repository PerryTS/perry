import * as https from "node:https";

function logDescriptor(name: string) {
  const descriptor = Object.getOwnPropertyDescriptor(https, name);
  console.log(
    name,
    descriptor
      ? [
        typeof descriptor.value,
        descriptor.enumerable,
        descriptor.configurable,
      ].join(" ")
      : "missing",
  );
}

logDescriptor("Agent");
logDescriptor("Server");
logDescriptor("createServer");
logDescriptor("get");
logDescriptor("globalAgent");
logDescriptor("request");
