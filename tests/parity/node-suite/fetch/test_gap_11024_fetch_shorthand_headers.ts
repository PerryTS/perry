import http from "node:http";

const server = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "text/plain" });
  res.end(String(req.headers["x-keep"] || "<absent>"));
});

async function main(): Promise<void> {
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", resolve);
  });

  const address = server.address() as any;
  const base = "http://127.0.0.1:" + address.port;
  const headers = new Headers();
  headers.set("x-keep", "yes");

  const explicit = await fetch(base + "/explicit", {
    method: "PUT",
    body: "a",
    headers: headers,
  });
  console.log("explicit", await explicit.text());

  const shorthand = await fetch(base + "/shorthand", {
    method: "PUT",
    body: "b",
    headers,
  });
  console.log("shorthand", await shorthand.text());

  await new Promise<void>((resolve) => server.close(() => resolve()));
}

main();
