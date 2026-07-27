import { createServer } from "node:http";

const server = createServer();
const events: string[] = [];
server.on("listening", () => events.push("listening"));
server.on("close", () => events.push("close"));
console.log("before:", server.listening, server.address());
const listening = server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  console.log(
    "during:",
    server.listening,
    typeof address === "object" && address !== null,
    typeof address === "object" && address !== null
      ? address.family
      : "missing",
  );
  console.log(
    "ref self:",
    server.ref() === server,
    server.unref() === server,
    server.ref() === server,
  );
  console.log("close self:", server.close() === server);
});
console.log("listen self:", listening === server);
server.on(
  "close",
  () =>
    console.log("after:", server.listening, server.address(), events.join("|")),
);
