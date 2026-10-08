import * as net from "node:net";
const server = net.createServer(() => {});
server.on("error", (error: any) => {
  console.log("denied", error.code === "EACCES" || error.code === "EPERM");
});
server.listen(0, "127.0.0.1", () => console.log("unexpected listen"));
