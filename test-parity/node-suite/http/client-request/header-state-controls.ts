import http from "node:http";

const req: any = http.request({
  host: "127.0.0.1",
  port: 9,
  path: "/p?q=1",
  method: "POST",
});

req.on("error", () => {});

function line(label: string, value: unknown) {
  console.log(`${label}:`, value);
}

line("header method types", [
  typeof req.setHeader,
  typeof req.getHeader,
  typeof req.hasHeader,
  typeof req.removeHeader,
  typeof req.getHeaderNames,
  typeof req.getHeaders,
  typeof req.getRawHeaderNames,
].join("|"));

line("setHeader returns self", req.setHeader("X-Foo", "bar") === req);
line("has x", req.hasHeader("x-foo"));
line("get x", req.getHeader("x-foo"));
line("names include x", req.getHeaderNames().includes("x-foo"));
line("headers x", req.getHeaders()["x-foo"]);
line("raw include original", req.getRawHeaderNames().includes("X-Foo"));
line("remove returns undefined", String(req.removeHeader("x-foo")));
line("has after remove", req.hasHeader("X-Foo"));
line("get after remove", String(req.getHeader("X-Foo")));

line("state defaults", [
  req.aborted,
  req.destroyed,
  req.finished,
  req.reusedSocket,
  req.maxHeadersCount === null,
  req.writableEnded,
  req.writableFinished,
].map(String).join("|"));

line("socket aliases", [
  typeof req.socket,
  typeof req.connection,
  req.socket === req.connection,
].join("|"));

line("control method types", [
  typeof req.abort,
  typeof req.destroy,
  typeof req.flushHeaders,
  typeof req.cork,
  typeof req.uncork,
  typeof req.setNoDelay,
  typeof req.setSocketKeepAlive,
].join("|"));

line("cork return", String(req.cork()));
line("uncork return", String(req.uncork()));
line("setNoDelay return", String(req.setNoDelay()));
line("setSocketKeepAlive return", String(req.setSocketKeepAlive()));
line("destroy returns self", req.destroy() === req);
line("state after destroy", [
  req.aborted,
  req.destroyed,
  req.writableEnded,
  req.writableFinished,
].map(String).join("|"));
