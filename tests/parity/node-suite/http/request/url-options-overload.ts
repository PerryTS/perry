import { createServer, request } from "node:http";

const seen: string[] = [];
const server = createServer((req, res) => {
  seen.push(`${req.method} ${req.url} ${req.headers["x-source"]}`);
  res.end("ok");
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = request(
      new URL(`http://127.0.0.1:${address.port}/from-url?one=1`),
      {
        method: "POST",
        path: "/from-options?two=2",
        headers: { "X-Source": "options" },
      },
      (res) => {
        res.once("error", reject);
        res.on("end", resolve);
        res.resume();
      },
    );
    req.once("error", reject);
    console.log("return:", req.constructor.name, req.method, req.path);
    req.end();
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
  console.log("seen:", seen.join("|"));
}
