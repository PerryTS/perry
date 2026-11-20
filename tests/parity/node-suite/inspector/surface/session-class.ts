import { EventEmitter } from "node:events";
import { Session } from "node:inspector";

const session = new Session();
console.log(
  "class:",
  Session.name,
  Session.length,
  session instanceof Session,
  session instanceof EventEmitter,
);
console.log(
  "prototype identity:",
  Object.getPrototypeOf(session) === Session.prototype,
);
console.log(
  "methods:",
  Object.getOwnPropertyNames(Session.prototype).sort().join(","),
);
for (
  const name of [
    "connect",
    "connectToMainThread",
    "disconnect",
    "post",
  ] as const
) {
  const fn = Session.prototype[name];
  const descriptor = Object.getOwnPropertyDescriptor(Session.prototype, name);
  console.log(
    name,
    fn.name,
    fn.length,
    descriptor?.enumerable,
    descriptor?.writable,
    descriptor?.configurable,
  );
}
