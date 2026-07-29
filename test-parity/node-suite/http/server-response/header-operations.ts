import { IncomingMessage, ServerResponse } from "node:http";

const response = new ServerResponse(new IncomingMessage(null as any));
console.log("set self:", response.setHeader("X-One", "a") === response);
console.log("append self:", response.appendHeader("X-One", "b") === response);
console.log("value:", String(response.getHeader("x-one")));
console.log("has:", response.hasHeader("X-ONE"));
console.log("names:", response.getHeaderNames().join("|"));
console.log("raw names:", response.getRawHeaderNames().join("|"));
console.log(
  "null prototype:",
  Object.getPrototypeOf(response.getHeaders()) === null,
);
console.log(
  "remove:",
  String(response.removeHeader("x-one")),
  response.hasHeader("x-one"),
);
