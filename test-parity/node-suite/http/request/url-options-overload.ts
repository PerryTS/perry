import { createServer, request } from "node:http";

const seen: string[] = [];
const server = createServer((req, res) => {
  seen.push(`${req.method} ${req.url} ${req.headers["x-source"]}`);
  res.end("ok", () => server.close(() => console.log("seen:", seen.join("|"))));
});

server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("missing address");
  }
  const req = request(
    new URL(`http://127.0.0.1:${address.port}/from-url?one=1`),
    {
      method: "POST",
      path: "/from-options?two=2",
      headers: { "X-Source": "options" },
    },
    (res) => res.resume(),
  );
  console.log("return:", req.constructor.name, req.method, req.path);
  req.end();
});
