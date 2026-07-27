import * as https from "node:https";

function logDescriptor(name: string) {
  const descriptor = Object.getOwnPropertyDescriptor(https, name)!;
  console.log(
    name,
    typeof descriptor.value,
    descriptor.enumerable,
    descriptor.configurable,
  );
}

logDescriptor("Agent");
logDescriptor("Server");
logDescriptor("createServer");
logDescriptor("get");
logDescriptor("globalAgent");
logDescriptor("request");
