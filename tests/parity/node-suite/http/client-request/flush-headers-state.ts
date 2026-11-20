import { request } from "node:http";

const req = request({
  host: "example.test",
  agent: { addRequest() {} } as any,
  method: "POST",
});
req.setHeader("X-Flushed", "yes");
console.log("before:", req.headersSent, req.finished);
console.log("return:", String(req.flushHeaders()));
console.log(
  "after:",
  req.headersSent,
  req.finished,
  req.getHeader("x-flushed"),
);
req.destroy();
