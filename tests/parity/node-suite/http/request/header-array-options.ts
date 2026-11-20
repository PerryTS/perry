import { createServer, request } from "node:http";

const seen: string[] = [];
const server = createServer((req, res) => {
  const kind = seen.length === 0 ? "object" : "array";
  seen.push([
    kind,
    req.headers["x-foo"],
    req.headers.cookie,
    req.headers.authorization ?? "<missing>",
    kind === "object"
      ? String(req.headers.host?.startsWith("127.0.0.1:"))
      : req.headers.host,
  ].join("|"));
  res.end("ok");
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("missing address");

async function send(options: Record<string, unknown>): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const req = request({
      hostname: "127.0.0.1",
      port: address.port,
      path: "/",
      ...options,
    }, (res) => {
      res.once("error", reject);
      res.on("end", resolve);
      res.resume();
    });
    req.once("error", reject);
    req.end();
  });
}

try {
  await send({
    auth: "foo:bar",
    headers: { "x-foo": "boom", cookie: ["a=1", "b=2", "c=3"] },
  });
  await send({
    auth: "foo:bar",
    headers: [
      ["x-foo", "boom"],
      ["cookie", "a=1"],
      ["cookie", ["b=2", "c=3"]],
      ["Host", "example.com"],
    ],
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

console.log(seen.join("\n"));
