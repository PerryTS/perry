import { Session } from "node:inspector/promises";

const descriptor = Object.getOwnPropertyDescriptor(Session.prototype, "post");
console.log(
  "descriptor:",
  descriptor?.enumerable,
  descriptor?.writable,
  descriptor?.configurable,
);
console.log(
  "function:",
  Session.prototype.post.name,
  Session.prototype.post.length,
  Object.prototype.toString.call(Session.prototype.post),
);
console.log(
  "own:",
  Object.hasOwn(Session.prototype, "post"),
  Object.hasOwn(Session.prototype, "connect"),
);
