// fetch(url, { body: asyncIterable, duplex: "half" }) must send the iterable's
// chunks as the request body.
import http from "node:http";

const enc = new TextEncoder();
async function* gen() {
  yield enc.encode("hello ");
  yield "async ";
  yield enc.encode("world");
}

const server = http.createServer((req, res) => {
  const chunks: Buffer[] = [];
  req.on("data", (c: Buffer) => chunks.push(c));
  req.on("end", () => res.end(`${req.method} ${Buffer.concat(chunks).toString()}`));
});

server.listen(0, "127.0.0.1", async () => {
  const { port } = server.address() as { port: number };
  try {
    const res = await fetch(`http://127.0.0.1:${port}/`, {
      method: "POST",
      // Cast only the body: a cast around the whole init literal is a
      // separate fetch-init lowering gap.
      body: gen() as any,
      duplex: "half",
    });
    console.log(res.status, await res.text());
  } finally {
    server.close();
  }
});
