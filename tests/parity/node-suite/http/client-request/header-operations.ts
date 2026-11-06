import { request } from "node:http";

const req = request({
  host: "example.test",
  agent: { addRequest() {} } as any,
  headers: { "X-First": "one" },
});
req.on("error", () => {});
console.log(
  "initial:",
  req.getHeader("x-first"),
  req.getRawHeaderNames().join("|"),
);
console.log("set self:", req.setHeader("X-Second", ["two", "three"]) === req);
console.log(
  "array identity:",
  Array.isArray(req.getHeader("x-second")),
  String(req.getHeader("x-second")),
);
console.log("names:", req.getHeaderNames().join("|"));
console.log(
  "null prototype:",
  Object.getPrototypeOf(req.getHeaders()) === null,
);
console.log(
  "remove:",
  String(req.removeHeader("X-FIRST")),
  req.hasHeader("x-first"),
);
req.destroy();
