import { createServer, get } from "node:http";

const server = createServer((_req, res) => {
  const events: string[] = [];
  res.on("finish", () => events.push("finish"));
  res.on("close", () => events.push("close"));
  console.log(
    "initial:",
    res.headersSent,
    res.finished,
    res.writableEnded,
    res.writableFinished,
  );
  console.log("write:", res.write("a"), res.headersSent, res.finished);
  res.end("b", () => {
    console.log(
      "callback:",
      res.finished,
      res.writableEnded,
      res.writableFinished,
    );
  });
  res.on("close", () => console.log("events:", events.join("|")));
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

try {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("missing address");
  }
  await new Promise<void>((resolve, reject) => {
    get({ hostname: "127.0.0.1", port: address.port }, (res) => {
      res.once("error", reject);
      res.resume();
      res.on("end", resolve);
    }).once("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
