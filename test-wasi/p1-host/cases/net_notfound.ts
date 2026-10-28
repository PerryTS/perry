// A name that is not found, reported after the lookup's delay: ENOTFOUND from
// getaddrinfo, not a connection error.
import * as net from "node:net";

const started = Date.now();
const socket = net.connect(9, "gone.test");
socket.on("connect", () => console.log("unexpected connect"));
socket.on("error", (error: any) => {
  console.log(error.code, error.syscall, Date.now() - started >= 150 ? "after delay" : "too early");
});
